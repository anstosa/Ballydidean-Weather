#!/usr/bin/env bash
set -euo pipefail

# fixed one-task release identities
readonly direct_source_release=2026.10.09-2
readonly direct_previous_release=2026.10.09-1
readonly direct_target_release=2026.11.03-2
readonly direct_source_env_sha=97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7
readonly direct_control_sha=6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb
readonly direct_revised_20_sha=58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6
readonly direct_target_history=3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554
readonly direct_retained_cap=20401094656
readonly direct_peak_cap=23622320128
readonly direct_installed_update=/opt/weather/current/deploy/scripts/update.sh
readonly direct_installed_update_sha=ea1185e04e0da2439c856b5b6989ce4f87f087a63ff7bafa1781c577cf42b8a0

direct_loaded=false
direct_mutation=false
direct_complete=false
direct_identity_sha=
direct_compatibility_proof=
direct_compatibility_sha=
direct_backup_sha=
direct_source_env=
direct_target_env=
direct_source_history=
direct_migration_name=weather_direct_18_23_2026_11_03_2

# print the closed phase interface
direct_usage() {
  cat <<'EOF'
Usage:
  direct-maintenance-rollout.sh prepare IDENTITY_JSON IDENTITY_SHA256
  direct-maintenance-rollout.sh compatibility IDENTITY_JSON IDENTITY_SHA256 PROOF_JSON
  direct-maintenance-rollout.sh backup IDENTITY_JSON IDENTITY_SHA256
  direct-maintenance-rollout.sh activate IDENTITY_JSON IDENTITY_SHA256 PROOF_JSON ACTIVATION_JSON ACTIVATION_SHA256
  direct-maintenance-rollout.sh recover IDENTITY_JSON IDENTITY_SHA256 PROOF_JSON
EOF
}

# require root for production phases
direct_require_root() {
  [[ "$EUID" == 0 ]] || { printf 'error: direct rollout requires root\n' >&2; return 1; }
}

# source the installed updater as a library
direct_load_control() {
  # avoid reloading the same fixed library
  if [[ "$direct_loaded" == true ]]; then
    return
  fi
  [[ -f "$direct_installed_update" && ! -L "$direct_installed_update" &&
    "$(stat -c '%u:%g:%a:%h' "$direct_installed_update")" == 0:0:755:1 &&
    "$(sha256sum "$direct_installed_update" | awk '{print $1}')" == \
      "$direct_installed_update_sha" ]] || {
    printf 'error: installed updater identity differs\n' >&2
    return 1
  }
  # shellcheck disable=SC1090,SC1091,SC2154
  source "$direct_installed_update"
  # shellcheck disable=SC2154
  [[ "$deploy_dir" == /opt/weather/current/deploy &&
    "$state_dir" == /opt/weather/current/deploy/state &&
    "$releases_dir" == /opt/weather/current/deploy/releases &&
    "$control_plane_version" == 17 &&
    "$(control_plane_digest)" == "$direct_control_sha" ]] ||
    die "direct rollout installed control identity differs"
  direct_loaded=true
}

# require one bounded root-private regular file
direct_require_private() {
  local path=$1
  local maximum=${2:-65536}
  [[ "$maximum" =~ ^[1-9][0-9]*$ && ${#maximum} -le 18 ]] ||
    die "direct rollout private file bound is invalid"
  [[ "$path" == /* && -f "$path" && ! -L "$path" &&
    "$(realpath -e -- "$path")" == "$path" &&
    "$(stat -c '%u:%g:%a:%h' -- "$path")" == "$EUID:$(id -g):600:1" &&
    "$(stat -c '%s' -- "$path")" -ge 2 &&
    "$(stat -c '%s' -- "$path")" -le "$maximum" ]] ||
    die "direct rollout private file metadata differs: $path"
}

# require one exact archive under the fixed backup root
direct_require_backup_archive() {
  local path=$1
  local expected_bytes=$2
  local maximum_bytes=$3
  [[ "$path" =~ ^/var/lib/weather/backups/weather-[0-9]{8}T[0-9]{6}Z-[A-Za-z0-9]+\.dump\.age$ &&
    "$expected_bytes" =~ ^[1-9][0-9]*$ && "$maximum_bytes" =~ ^[1-9][0-9]*$ &&
    "$expected_bytes" -le "$maximum_bytes" ]] || die "direct backup archive identity differs"
  direct_require_private "$path" "$maximum_bytes"
  [[ "$(stat -c '%s' -- "$path")" == "$expected_bytes" ]] ||
    die "direct backup archive size differs"
}

# bind one private file to an external digest
direct_require_sha() {
  local path=$1
  local expected=$2
  [[ "$expected" =~ ^[a-f0-9]{64}$ ]] || die "direct rollout digest is invalid"
  direct_require_private "$path"
  [[ "$(sha256sum "$path" | awk '{print $1}')" == "$expected" ]] ||
    die "direct rollout private file digest differs: $path"
}

# read one bounded scalar JSON value
direct_json() {
  node --input-type=module - "$1" "$2" <<'NODE'
import { readFileSync } from "node:fs";
const [path, key] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"))[key];
// expose only single-line scalars
if (!["string", "number", "boolean"].includes(typeof value) || String(value).includes("\n")) {
  throw new Error(`invalid scalar: ${key}`);
}
process.stdout.write(String(value));
NODE
}

# validate the exact immutable publication identity
direct_validate_identity() {
  local path=$1
  local sha=$2
  direct_require_sha "$path" "$sha"
  node --input-type=module - "$path" "$direct_source_env_sha" "$direct_control_sha" \
    "$direct_revised_20_sha" "$direct_target_history" <<'NODE'
import { readFileSync } from "node:fs";
const [path, sourceEnv, control, revised20, targetHistory] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"));
const keys = ["checkRunId", "cloudflaredImage", "contractVersion", "controlPlaneSha256",
  "postgresImage", "previousRelease", "publishRunId", "revised20Sha256", "serverImage",
  "sourceEnvironmentSha256", "sourceRelease", "state", "targetCommit",
  "targetHistorySha256", "targetRelease", "webImage"];
const image = /^[^@\s]+@sha256:[a-f0-9]{64}$/u;
// admit only the externally proven fixed publication
if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(keys.sort()) ||
  value.contractVersion !== "weather-direct-maintenance-target/v1" ||
  value.sourceRelease !== "2026.10.09-2" || value.previousRelease !== "2026.10.09-1" ||
  value.targetRelease !== "2026.11.03-2" || value.sourceEnvironmentSha256 !== sourceEnv ||
  value.controlPlaneSha256 !== control || value.revised20Sha256 !== revised20 ||
  value.targetHistorySha256 !== targetHistory || value.state !== "publication_ready" ||
  !/^[a-f0-9]{40}$/u.test(value.targetCommit) ||
  !Number.isSafeInteger(value.checkRunId) || value.checkRunId < 1 ||
  !Number.isSafeInteger(value.publishRunId) || value.publishRunId < 1 ||
  ![value.serverImage, value.webImage, value.postgresImage, value.cloudflaredImage].every(
    (entry) => image.test(entry),
  )) throw new Error("direct rollout target identity differs");
NODE
}

# validate the sibling helper's genuine bounded compatibility proof
direct_validate_compatibility() {
  local proof=$1
  local target authorization
  direct_require_private "$proof"
  node --input-type=module - "$proof" "$direct_target_history" <<'NODE'
import { readFileSync } from "node:fs";
const [path, targetHistory] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"));
const keys = ["authorizationSha256", "capturedAt", "contractVersion", "fixtureBytes",
  "fullSizeRehearsal", "sourceApiAndIngestPassed", "sourceEnvironmentSha256",
  "sourceRelease", "sourceRuntimeTargetHistoryCompatible", "state",
  "targetEnvironmentSha256", "targetHistorySha256", "targetRelease",
  "unprovenHistoryRejected", "wrongHistoryRejected"];
// require genuine 64 MiB source-runtime compatibility, never a rehearsal alias
if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(keys.sort()) ||
  value.contractVersion !== "weather-direct-maintenance-source-compatibility/v1" ||
  value.sourceRelease !== "2026.10.09-2" || value.targetRelease !== "2026.11.03-2" ||
  value.targetHistorySha256 !== targetHistory || value.fixtureBytes !== 67_108_864 ||
  value.sourceRuntimeTargetHistoryCompatible !== true ||
  value.fullSizeRehearsal !== "explicitly-waived" || value.sourceApiAndIngestPassed !== true ||
  value.wrongHistoryRejected !== true || value.unprovenHistoryRejected !== true ||
  !/^[a-f0-9]{64}$/u.test(value.sourceEnvironmentSha256) ||
  !/^[a-f0-9]{64}$/u.test(value.targetEnvironmentSha256) ||
  !/^[a-f0-9]{64}$/u.test(value.authorizationSha256) ||
  value.state !== "compatibility_ready") {
  throw new Error("direct rollout compatibility proof differs");
}
NODE
  target=$(direct_target_environment)
  authorization=$(migration_authorization "$direct_target_release")
  [[ "$(direct_json "$proof" sourceEnvironmentSha256)" == "$direct_source_env_sha" &&
    "$(direct_json "$proof" targetEnvironmentSha256)" == \
      "$(sha256sum "$target" | awk '{print $1}')" &&
    "$(direct_json "$proof" authorizationSha256)" == \
      "$(sha256sum "$authorization" | awk '{print $1}')" ]] ||
    die "direct rollout compatibility proof identity differs"
}

# locate the fixed private operation state
direct_state_root() {
  printf '%s/direct-maintenance-%s\n' "$state_dir" "$direct_target_release"
}

# locate the fixed target environment
direct_target_environment() {
  printf '%s/%s.env\n' "$releases_dir" "$direct_target_release"
}

# begin one serialized production phase
direct_begin() {
  direct_require_root
  direct_load_control
  acquire_release_transaction_lock
  require_no_dashboard_override
  require_adjustment_family_fixed_roots
  require_command docker
  require_command dd
  require_command node
  install -d -m 0700 -o 0 -g 0 "$(direct_state_root)"
}

# require the exact active schema-18 source
direct_require_source() {
  local source
  source=$(normal_release_source_env)
  [[ "$(read_release_state "$state_dir/current-release")" == "$direct_source_release" &&
    "$(read_release_state "$state_dir/schema-release")" == "$direct_source_release" &&
    "$(read_optional_release_state "$state_dir/previous-release")" == "$direct_previous_release" &&
    "$(sha256sum "$source" | awk '{print $1}')" == "$direct_source_env_sha" ]] ||
    die "direct rollout source identity differs"
  printf '%s\n' "$source"
}

# require actual free bytes without deletion credit
direct_require_free() {
  local path=$1
  local required=$2
  local blocks size
  read -r blocks size < <(stat -f -c '%a %S' -- "$path")
  ((blocks * size >= required)) || die "direct rollout actual free space is too low"
}

# verify the rendered target environment
direct_require_target() {
  local target=$1
  local identity=$2
  local source key
  source=$(release_env "$direct_source_release")
  validate_release_env "$target" "$direct_target_release"
  [[ "$(env_value "$target" WEATHER_SERVER_IMAGE)" == "$(direct_json "$identity" serverImage)" &&
    "$(env_value "$target" WEATHER_WEB_IMAGE)" == "$(direct_json "$identity" webImage)" &&
    "$(env_value "$target" POSTGRES_IMAGE)" == "$(direct_json "$identity" postgresImage)" &&
    "$(env_value "$target" CLOUDFLARED_IMAGE)" == "$(direct_json "$identity" cloudflaredImage)" &&
    "$(env_value "$target" WEATHER_CONTROL_PLANE_VERSION)" == 17 &&
    "$(env_value "$target" WEATHER_CONTROL_PLANE_SHA256)" == "$direct_control_sha" ]] ||
    die "direct rollout target environment differs"
  # preserve every source-owned mutable setting
  for key in WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH; do
    [[ "$(env_value "$target" "$key")" == "$(env_value "$source" "$key")" ]] ||
      die "direct rollout changed source setting: $key"
  done
}

# prepare digest-pinned images without clone or stage receipts
direct_prepare() {
  local identity=$1
  local identity_sha=$2
  local source target temporary image
  local -a images
  direct_begin
  direct_validate_identity "$identity" "$identity_sha"
  source=$(direct_require_source)
  target=$(direct_target_environment)
  require_normal_release_resource_gate
  require_image_pull_capacity_floor
  direct_require_free /var/lib/weather 2147483648
  temporary=$(mktemp "$releases_dir/.${direct_target_release}.XXXXXX.env")
  # remove only an unpublished temporary environment
  trap '[[ -z "$temporary" ]] || rm -f -- "$temporary"' EXIT
  write_release_env "$source" "$temporary" "$direct_target_release" \
    "$(direct_json "$identity" serverImage)" "$(direct_json "$identity" webImage)" \
    "$(direct_json "$identity" postgresImage)" "$(direct_json "$identity" cloudflaredImage)"
  # reuse only a byte-identical target
  if [[ -e "$target" || -L "$target" ]]; then
    [[ -f "$target" && ! -L "$target" ]] || die "direct rollout target environment is unsafe"
    cmp -s "$temporary" "$target" || die "direct rollout target environment differs"
    rm -f -- "$temporary"
    temporary=
  else
    mv -- "$temporary" "$target"
    temporary=
    sync -f "$releases_dir"
  fi
  direct_require_target "$target" "$identity"
  require_inert_v14_empty_catalog
  WEATHER_ENV_FILE=$target compose config --quiet
  cleanup_obsolete_weather_images "$source" "$target"
  require_image_pull_capacity_floor
  WEATHER_ENV_FILE=$target compose pull
  images=("$(direct_json "$identity" serverImage)" "$(direct_json "$identity" webImage)"
    "$(direct_json "$identity" postgresImage)" "$(direct_json "$identity" cloudflaredImage)")
  # verify every local image is the ARM64 digest target
  for image in "${images[@]}"; do
    [[ "$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")" == linux/arm64 ]] ||
      die "direct rollout image platform differs: $image"
  done
  trap - EXIT
  printf 'Prepared %s without full-size clone rehearsal.\n' "$direct_target_release"
}

# run the dedicated genuine compatibility helper
direct_compatibility() {
  local identity=$1
  local identity_sha=$2
  local proof=$3
  local target authorization helper
  direct_begin
  direct_validate_identity "$identity" "$identity_sha"
  direct_require_source >/dev/null
  target=$(direct_target_environment)
  direct_require_target "$target" "$identity"
  authorization=$(migration_authorization "$direct_target_release")
  [[ "$proof" == "$releases_dir/$direct_target_release.direct-compatibility.json" ]] ||
    die "direct compatibility proof path differs"
  helper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/direct-maintenance-compatibility.sh"
  [[ -x "$helper" && ! -L "$helper" ]] || die "direct compatibility helper is unavailable"
  # never overwrite a success proof through a second fixture attempt
  if [[ -e "$proof" || -L "$proof" ]]; then
    direct_validate_compatibility "$proof"
    validate_migration_authorization "$authorization" "$direct_source_release" \
      "$direct_target_release"
    printf 'Reused genuine compatibility proof %s.\n' "$proof"
    return
  fi
  # shellcheck disable=SC2154
  export WEATHER_RELEASE_LOCK_FD=$release_transaction_lock_fd
  "$helper" "$target" "$authorization" "$proof"
  direct_validate_compatibility "$proof"
  validate_migration_authorization "$authorization" "$direct_source_release" \
    "$direct_target_release"
  [[ "$(env_value "$authorization" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)" == \
    "$direct_target_history" ]] || die "direct compatibility authorization history differs"
  printf 'Accepted genuine 64 MiB compatibility proof.\n'
}

# locate the single backup receipt
direct_backup_receipt() {
  printf '%s/backup-receipt.json\n' "$(direct_state_root)"
}

# atomically publish one private JSON record
direct_publish_json() {
  local target=$1
  local body=$2
  local temporary
  temporary=$(mktemp "$(dirname "$target")/.$(basename "$target").XXXXXX")
  chmod 600 "$temporary"
  printf '%s\n' "$body" >"$temporary"
  sync -f "$temporary"
  mv -- "$temporary" "$target"
  chmod 600 "$target"
  sync -f "$(dirname "$target")"
}

# validate the exact encrypted backup publication
direct_validate_backup() {
  local receipt=$1
  local identity_sha=$2
  local archive checksum expected archive_bytes maximum_bytes allocated_bytes actual_allocated
  direct_require_private "$receipt"
  node --input-type=module - "$receipt" "$identity_sha" <<'NODE'
import { readFileSync } from "node:fs";
const [path, identitySha] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"));
const keys = ["allocatedBytes", "archive", "archiveBytes", "archiveSha256", "capturedAt",
  "checksum", "contractVersion", "identitySha256", "logicalDatabaseBytes",
  "maxOutputBytes", "sourceRelease", "state", "targetRelease"];
// bind one backup invocation to one encrypted archive pair
if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(keys.sort()) ||
  value.contractVersion !== "weather-direct-maintenance-backup/v1" ||
  value.identitySha256 !== identitySha || value.sourceRelease !== "2026.10.09-2" ||
  value.targetRelease !== "2026.11.03-2" || value.state !== "published" ||
  !value.archive.startsWith("/var/lib/weather/backups/weather-") ||
  !value.archive.endsWith(".dump.age") || value.checksum !== `${value.archive}.sha256` ||
  !/^[a-f0-9]{64}$/u.test(value.archiveSha256) ||
  !Number.isSafeInteger(value.logicalDatabaseBytes) || value.logicalDatabaseBytes < 1 ||
  value.maxOutputBytes !== value.logicalDatabaseBytes + 1_073_741_824 ||
  !Number.isSafeInteger(value.archiveBytes) || value.archiveBytes < 1 ||
  value.archiveBytes > value.maxOutputBytes || !Number.isSafeInteger(value.allocatedBytes) ||
  value.allocatedBytes < value.archiveBytes || !Number.isFinite(Date.parse(value.capturedAt))) {
  throw new Error("direct rollout backup receipt differs");
}
NODE
  archive=$(direct_json "$receipt" archive)
  checksum=$(direct_json "$receipt" checksum)
  archive_bytes=$(direct_json "$receipt" archiveBytes)
  maximum_bytes=$(direct_json "$receipt" maxOutputBytes)
  allocated_bytes=$(direct_json "$receipt" allocatedBytes)
  direct_require_backup_archive "$archive" "$archive_bytes" "$maximum_bytes"
  direct_require_private "$checksum"
  actual_allocated=$(( ($(stat -c '%b' "$archive") + $(stat -c '%b' "$checksum")) * 512 ))
  [[ "$actual_allocated" == "$allocated_bytes" ]] ||
    die "direct rollout backup allocated-byte accounting differs"
  expected=$(direct_json "$receipt" archiveSha256)
  [[ "$(sha256sum "$archive" | awk '{print $1}')" == "$expected" ]] ||
    die "direct rollout backup digest differs"
  (cd "$(dirname "$archive")" && sha256sum --check "$(basename "$checksum")") >/dev/null ||
    die "direct rollout backup checksum differs"
}

# invoke the encrypted backup at most once
direct_backup() {
  local identity=$1
  local identity_sha=$2
  local source database logical maximum required root attempt receipt output archive checksum
  local archive_sha archive_bytes allocated captured body
  direct_begin
  direct_validate_identity "$identity" "$identity_sha"
  source=$(direct_require_source)
  direct_require_target "$(direct_target_environment)" "$identity"
  root=$(direct_state_root)
  attempt="$root/backup-attempt.json"
  receipt=$(direct_backup_receipt)
  # reuse only a complete verified backup
  if [[ -e "$receipt" || -L "$receipt" ]]; then
    direct_validate_backup "$receipt" "$identity_sha"
    printf 'Reused verified backup receipt %s.\n' "$receipt"
    return
  fi
  # never repeat an uncertain or failed backup attempt
  if [[ -e "$attempt" || -L "$attempt" ]]; then
    die "direct rollout backup was already attempted without a receipt"
  fi
  require_normal_release_resource_gate
  database=$(env_value "$source" WEATHER_DATABASE_NAME)
  logical=$(WEATHER_ENV_FILE=$source compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database" \
    --tuples-only --no-align --command 'SELECT pg_database_size(current_database())')
  [[ "$logical" =~ ^[1-9][0-9]*$ && ${#logical} -le 18 ]] ||
    die "direct rollout logical database size differs"
  maximum=$((logical + 1073741824))
  required=$((maximum + 2147483648))
  direct_require_free /var/lib/weather "$required"
  captured=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  body=$(node --input-type=module -e '
const [identitySha256, capturedAt] = process.argv.slice(1);
process.stdout.write(JSON.stringify({capturedAt,
  contractVersion:"weather-direct-maintenance-backup-attempt/v1", identitySha256,
  sourceRelease:"2026.10.09-2", state:"started", targetRelease:"2026.11.03-2"}));
' "$identity_sha" "$captured")
  direct_publish_json "$attempt" "$body"
  output=$(/opt/weather/current/deploy/scripts/backup.sh --env-file "$source" \
    --max-output-bytes "$maximum")
  archive=$(sed -n 's/^Backup complete: //p' <<<"$output")
  checksum=$(sed -n 's/^Checksum: //p' <<<"$output")
  [[ "$archive" == /var/lib/weather/backups/weather-*.dump.age &&
    "$checksum" == "$archive.sha256" ]] || die "direct rollout backup output differs"
  direct_require_private "$checksum"
  (cd "$(dirname "$archive")" && sha256sum --check "$(basename "$checksum")") >/dev/null ||
    die "direct rollout backup checksum differs"
  archive_sha=$(sha256sum "$archive" | awk '{print $1}')
  archive_bytes=$(stat -c '%s' "$archive")
  direct_require_backup_archive "$archive" "$archive_bytes" "$maximum"
  allocated=$(( ($(stat -c '%b' "$archive") + $(stat -c '%b' "$checksum")) * 512 ))
  ((archive_bytes <= maximum && allocated >= archive_bytes)) ||
    die "direct rollout backup accounting differs"
  captured=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  body=$(node --input-type=module -e '
const [identitySha256, archive, checksum, archiveSha256, logicalDatabaseBytes,
  maxOutputBytes, archiveBytes, allocatedBytes, capturedAt] = process.argv.slice(1);
process.stdout.write(JSON.stringify({allocatedBytes:Number(allocatedBytes), archive,
  archiveBytes:Number(archiveBytes), archiveSha256, capturedAt, checksum,
  contractVersion:"weather-direct-maintenance-backup/v1", identitySha256,
  logicalDatabaseBytes:Number(logicalDatabaseBytes), maxOutputBytes:Number(maxOutputBytes),
  sourceRelease:"2026.10.09-2", state:"published", targetRelease:"2026.11.03-2"}));
' "$identity_sha" "$archive" "$checksum" "$archive_sha" "$logical" "$maximum" \
    "$archive_bytes" "$allocated" "$captured")
  direct_publish_json "$receipt" "$body"
  direct_validate_backup "$receipt" "$identity_sha"
  printf 'Published one encrypted direct rollout backup: %s\n' "$archive"
}

# validate fresh one-backup external cap authorization
direct_validate_activation() {
  local path=$1
  local sha=$2
  local identity_sha=$3
  local backup_sha=$4
  direct_require_sha "$path" "$sha"
  node --input-type=module - "$path" "$identity_sha" "$backup_sha" \
    "$direct_retained_cap" "$direct_peak_cap" <<'NODE'
import { readFileSync } from "node:fs";
const [path, identitySha, backupSha, retainedCap, peakCap] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"));
const keys = ["authorized", "backupPublications", "backupReceiptSha256", "capturedAt",
  "contractVersion", "identitySha256", "peakBytesMaximum", "retainedBytesMaximum",
  "sourceRelease", "state", "targetRelease"];
const captured = Date.parse(value.capturedAt);
const age = Date.now() - captured;
// admit only the fresh exact external ledger decision
if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(keys.sort()) ||
  value.contractVersion !== "weather-direct-maintenance-activation/v1" ||
  value.identitySha256 !== identitySha || value.backupReceiptSha256 !== backupSha ||
  value.backupPublications !== 1 || value.retainedBytesMaximum !== Number(retainedCap) ||
  value.peakBytesMaximum !== Number(peakCap) || value.authorized !== true ||
  value.sourceRelease !== "2026.10.09-2" || value.targetRelease !== "2026.11.03-2" ||
  value.state !== "activation_ready" || !Number.isFinite(captured) ||
  age < -300_000 || age > 900_000) throw new Error("activation authorization differs");
NODE
}

# persist a root-private fsynced migration journal
direct_journal() {
  local state=$1
  local container=${2:-absent}
  local exit_code=${3:-absent}
  local target body captured
  [[ "$state" == prepared || "$state" == running || "$state" == launch_failed ||
    "$state" == removed || "$state" == committed || "$state" == target_healthy ||
    "$state" == recovered_source || "$state" == recovered_compatible_source ]] ||
    die "direct rollout journal state differs"
  target="$(direct_state_root)/activation-journal.json"
  captured=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  body=$(node --input-type=module -e '
const [state, containerId, exitCode, capturedAt, identitySha256, migrationName] =
  process.argv.slice(1);
const sourceHistorySha256 = process.argv[7];
const compatibilityProofSha256 = process.argv[8];
const backupReceiptSha256 = process.argv[9];
process.stdout.write(JSON.stringify({capturedAt, containerId,
  backupReceiptSha256, compatibilityProofSha256,
  contractVersion:"weather-direct-maintenance-journal/v1", exitCode, identitySha256,
  migrationName, sourceHistorySha256, targetHistorySha256:
    "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554",
  sourceRelease:"2026.10.09-2", state, targetRelease:"2026.11.03-2"}));
' "$state" "$container" "$exit_code" "$captured" "$direct_identity_sha" \
    "$direct_migration_name" "$direct_source_history" "$direct_compatibility_sha" \
    "$direct_backup_sha")
  direct_publish_json "$target" "$body"
}

# validate the root-private journal used after restart
direct_validate_journal() {
  local journal=$1
  local identity_sha=$2
  local compatibility_sha=$3
  local backup_sha=$4
  direct_require_private "$journal"
  node --input-type=module - "$journal" "$identity_sha" "$compatibility_sha" \
    "$backup_sha" "$direct_target_history" <<'NODE'
import { readFileSync } from "node:fs";
const [path, identitySha, compatibilitySha, backupSha, targetHistory] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"));
const keys = ["backupReceiptSha256", "capturedAt", "compatibilityProofSha256", "containerId",
  "contractVersion", "exitCode", "identitySha256", "migrationName", "sourceHistorySha256",
  "sourceRelease", "state", "targetHistorySha256", "targetRelease"];
const states = new Set(["prepared", "running", "launch_failed", "removed", "committed",
  "target_healthy", "recovered_source", "recovered_compatible_source"]);
// admit only this operation's exact durable journal
if (JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(keys.sort()) ||
  value.contractVersion !== "weather-direct-maintenance-journal/v1" ||
  value.identitySha256 !== identitySha || value.sourceRelease !== "2026.10.09-2" ||
  value.targetRelease !== "2026.11.03-2" ||
  value.migrationName !== "weather_direct_18_23_2026_11_03_2" ||
  value.compatibilityProofSha256 !== compatibilitySha ||
  value.backupReceiptSha256 !== backupSha || value.targetHistorySha256 !== targetHistory ||
  !states.has(value.state) || !/^[a-f0-9]{64}$/u.test(value.sourceHistorySha256)) {
  throw new Error("direct rollout journal differs");
}
NODE
}

# validate the one exact named migration container
direct_require_migration_container() {
  local container=$1
  local target=$2
  local reference image_id details
  reference=$(env_value "$target" WEATHER_SERVER_IMAGE)
  image_id=$(docker image inspect --format '{{.Id}}' "$reference")
  details=$(docker container inspect --format \
    '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
    "$container")
  [[ "$details" == "$container|/$direct_migration_name|weather|migration|$reference|$image_id" ]] ||
    die "direct rollout migration container identity differs"
}

# retain bounded combined migration output before container removal
direct_capture_migration_logs() {
  local container=$1
  local root target temporary size
  root=$(direct_state_root)
  target="$root/migration-$container.log"
  # reuse only an already captured bounded root-private log
  if [[ -e "$target" || -L "$target" ]]; then
    [[ -f "$target" && ! -L "$target" && "$(realpath -e -- "$target")" == "$target" &&
      "$(stat -c '%u:%g:%a:%h' -- "$target")" == "$EUID:$(id -g):600:1" &&
      "$(stat -c '%s' -- "$target")" -le 16777216 ]] ||
      die "direct migration retained log differs"
    return
  fi
  temporary=$(mktemp "$root/.migration-$container.XXXXXX.log")
  chmod 600 "$temporary"
  # retain the first 16 MiB while draining the exact container log stream
  if ! docker container logs "$container" 2>&1 | {
    dd bs=1048576 count=16 of="$temporary" status=none
    cat >/dev/null
  }; then
    rm -f -- "$temporary"
    return 1
  fi
  size=$(stat -c '%s' "$temporary")
  ((size <= 16777216)) || die "direct migration retained log exceeded its bound"
  sync -f "$temporary"
  mv -- "$temporary" "$target"
  chmod 600 "$target"
  sync -f "$root"
}

# require no migration-owner sessions or prepared transactions
direct_require_database_quiesced() {
  local environment=$1
  local database result
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  result=$(WEATHER_ENV_FILE=$environment compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database" \
    --tuples-only --no-align --command \
    "SELECT (NOT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid <> pg_backend_pid() AND usename = 'weather_owner') AND NOT EXISTS (SELECT 1 FROM pg_prepared_xacts WHERE database = current_database()))::integer")
  [[ "$result" == 1 ]] || die "direct migration owner sessions or prepared transactions remain"
}

# create the exact v17 capture epoch from the live schema-23 envelope
direct_initialize_capture_epoch() {
  local identity=$1
  local environment=$2
  local database lock_root wrapper epoch_payload_sql
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  lock_root=/run/lock/weather
  wrapper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/direct-maintenance-epoch.mjs"
  [[ -f "$wrapper" && ! -L "$wrapper" &&
    "$(stat -c '%u:%g:%a:%h' "$wrapper")" == 0:0:755:1 ]] ||
    die "direct epoch wrapper identity differs"
  install -d -o 0 -g 0 -m 0700 "$lock_root"
  [[ ! -L "$lock_root" ]] || die "direct epoch lock root is linked"
  exec {direct_epoch_lock_fd}>"$lock_root/adjustment-capture-epoch.lock"
  chmod 0600 "$lock_root/adjustment-capture-epoch.lock"
  flock -n "$direct_epoch_lock_fd" || die "another capture epoch initialization is in flight"
  epoch_payload_sql='adjustment_revision_serving_snapshot_v1(transaction_timestamp(), 0)'
  # immutable retries validate the retained zero proof
  if [[ -e "$state_dir/adjustment-capture-epoch.json" ||
    -e "$state_dir/adjustment-capture-epoch-snapshot.json" ]]; then
    epoch_payload_sql='NULL::jsonb'
  fi
  # preserve the reviewed read-only database envelope and timeouts
  # shellcheck disable=SC2016
  if ! WEATHER_ENV_FILE=$environment compose exec -T postgres \
    sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_training_export_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --host 127.0.0.1 --username weather_training_export --dbname "$1"' \
    direct-capture-epoch "$database" <<SQL |
\set ON_ERROR_STOP on
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '5min';
SET LOCAL lock_timeout = '5s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
COPY (
  SELECT jsonb_build_object(
    'databaseManifest', to_jsonb(manifest),
    'payload', $epoch_payload_sql,
    'transaction', jsonb_build_object(
      'created_at_utc', to_char(transaction_timestamp() AT TIME ZONE 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
      'idle_in_transaction_session_timeout',
        current_setting('idle_in_transaction_session_timeout'),
      'isolation_level', current_setting('transaction_isolation'),
      'lock_timeout', current_setting('lock_timeout'),
      'read_only', current_setting('transaction_read_only'),
      'statement_timeout', current_setting('statement_timeout')
    )
  )::text
  FROM adjustment_evaluation_export_manifest_v1 manifest
) TO STDOUT WITH (FORMAT csv, DELIMITER E'\x01', QUOTE E'\x02', ESCAPE E'\x02');
COMMIT;
SQL
    node --max-old-space-size=192 --max-semi-space-size=4 "$wrapper" \
      "$identity" "$direct_identity_sha"
  then
    die "direct adjustment revision capture epoch initialization failed"
  fi
}

# quiesce and remove only the exact migration container
direct_quiesce_migration() {
  local target=$1
  local ids running
  ids=$(docker ps --all --no-trunc --quiet --filter "name=^/${direct_migration_name}$")
  [[ -z "$ids" || "$ids" =~ ^[a-f0-9]{64}$ ]] ||
    die "direct rollout migration container cardinality differs"
  # leave an absent exact container unchanged
  if [[ -z "$ids" ]]; then
    return
  fi
  direct_require_migration_container "$ids" "$target"
  running=$(docker container inspect --format '{{.State.Running}}' "$ids")
  # stop only a still-running exact container
  if [[ "$running" == true ]]; then
    docker container stop --time 30 "$ids" >/dev/null || return 1
  fi
  direct_capture_migration_logs "$ids" || return 1
  docker container rm "$ids" >/dev/null || return 1
}

# run the detached atomic migration through its terminal state
direct_run_migration() {
  local target=$1
  local output launch_status ids terminal wait_result
  launch_status=0
  # preserve partial launch state for exact-name reconciliation
  output=$(WEATHER_ENV_FILE=$target compose run --detach --no-deps \
    --name "$direct_migration_name" migration \
    node deploy/scripts/migrate.mjs --atomic-direct-18-23 2>&1) || launch_status=$?
  ids=$(docker ps --all --no-trunc --quiet --filter "name=^/${direct_migration_name}$")
  # classify a launch failure only after proving no container exists
  if [[ -z "$ids" ]]; then
    DIRECT_MIGRATION_EXIT=$launch_status
    [[ "$DIRECT_MIGRATION_EXIT" != 0 ]] || DIRECT_MIGRATION_EXIT=125
    direct_journal launch_failed absent "$DIRECT_MIGRATION_EXIT"
    require_supported_writers_quiesced "$target"
    return
  fi
  [[ "$ids" =~ ^[a-f0-9]{64}$ ]] || die "direct migration launch cardinality differs"
  direct_require_migration_container "$ids" "$target"
  direct_journal running "$ids" absent
  wait_result=$(docker container wait "$ids")
  [[ "$wait_result" =~ ^[0-9]{1,3}$ ]] || die "direct migration wait result differs"
  terminal=$(docker container inspect --format '{{.State.Status}}|{{.State.ExitCode}}' "$ids")
  [[ "$terminal" == "exited|$wait_result" || "$terminal" == "dead|$wait_result" ]] ||
    die "direct migration terminal state differs"
  direct_capture_migration_logs "$ids"
  docker container rm "$ids" >/dev/null
  direct_journal removed "$ids" "$wait_result"
  require_supported_writers_quiesced "$target"
  DIRECT_MIGRATION_EXIT=$wait_result
  : "$output"
}

# classify only the captured source or fixed revised target ledger
direct_classify() {
  local environment=$1
  local source_history=$2
  local database actual
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  actual=$(migration_history_sha256 "$environment" "$database")
  # reject every partial or unexpected ledger
  if [[ "$actual" == "$source_history" ]]; then
    printf 'source\n'
  elif [[ "$actual" == "$direct_target_history" ]]; then
    printf 'target\n'
  else
    printf 'unknown\n'
  fi
}

# initialize fixed gauges without the old schema-21 assertion
direct_initialize_gauges() {
  local environment=$1
  local database
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  # keep the owner password inside the database container
  # shellcheck disable=SC2016
  node --max-old-space-size=48 --max-semi-space-size=1 \
    "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    rain-fixed-gauge-target-sources-sql-v1 | \
    WEATHER_ENV_FILE=$environment compose exec -T postgres \
    sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
    direct-rain-target-sources "$database" | \
    node --max-old-space-size=48 --max-semi-space-size=1 \
    "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    project-rain-fixed-gauge-target-sources-v1
}

# publish target authority and commit release markers last
direct_finish_target() {
  local identity=$1
  local target=$2
  local database snapshot settings_inode settings_sha catalog_sha classification
  database=$(env_value "$target" WEATHER_DATABASE_NAME)
  apply_runtime_database_acl "$target" "$database"
  verify_runtime_database_acl "$target" "$database"
  direct_initialize_capture_epoch "$identity" "$target"
  direct_initialize_gauges "$target"
  require_inert_v14_empty_catalog
  snapshot=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    inert-v14-settings-snapshot)
  read -r settings_inode settings_sha <<<"$snapshot"
  [[ "$settings_inode" =~ ^[0-9]+:[0-9]+$ && "$settings_sha" =~ ^[a-f0-9]{64}$ ]] ||
    die "direct rollout settings snapshot differs"
  catalog_sha=$(sha256sum "$state_dir/adjustment-candidate-catalog.json" | awk '{print $1}')
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" bootstrap-inert-v14-current \
    "$direct_target_release" "$(direct_json "$identity" targetCommit)" \
    "$(env_value "$target" WEATHER_SERVER_IMAGE)" "$(env_value "$target" WEATHER_WEB_IMAGE)" \
    "$catalog_sha" "$settings_sha" >/dev/null
  start_exact_release "$target"
  classification=$(direct_classify "$target" impossible)
  [[ "$classification" == target ]] || die "direct target ledger changed before publication"
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    verify-inert-v14-settings-snapshot "$settings_inode" "$settings_sha" >/dev/null
  direct_journal target_healthy absent "$DIRECT_MIGRATION_EXIT"
  # release markers are the final deployment writes after health
  write_private_state "$state_dir/schema-release" "$direct_target_release"
  write_private_state "$state_dir/previous-release" "$direct_source_release"
  write_active_symlink "$direct_target_release"
  write_private_state "$state_dir/current-release" "$direct_target_release"
}

# recover only an exact rollback or compatibility-proven commit
direct_recover_inner() {
  local classification database authorization outcome=1
  set +e
  # never classify while container or transaction quiescence is uncertain
  if ! direct_quiesce_migration "$direct_target_env"; then
    printf 'error: direct migration container could not be quiesced; preserving evidence\n' >&2
    set -e
    return 1
  fi
  if ! start_postgres "$direct_target_env" ||
    ! require_supported_writers_quiesced "$direct_target_env" ||
    ! direct_require_database_quiesced "$direct_target_env"; then
    printf 'error: direct database quiescence is uncertain; preserving evidence\n' >&2
    set -e
    return 1
  fi
  classification=$(direct_classify "$direct_target_env" "$direct_source_history")
  # restart the pre-epoch runtime only after a full rollback
  if [[ "$classification" == source ]]; then
    if restore_v14_compatibility_bridge_images "$direct_source_env" \
      "$direct_source_release" "$direct_source_release"; then
      direct_journal recovered_source absent "${DIRECT_MIGRATION_EXIT:-unknown}"
      write_private_state "$state_dir/schema-release" "$direct_source_release"
      write_private_state "$state_dir/previous-release" "$direct_previous_release"
      write_active_symlink "$direct_source_release"
      write_private_state "$state_dir/current-release" "$direct_source_release"
      printf 'Direct rollout recovered unchanged schema 18.\n' >&2
      outcome=0
    else
      printf 'error: schema-18 runtime recovery failed; preserving evidence\n' >&2
    fi
  # use the old runtime on schema 23 only with the genuine proof and authorization
  elif [[ "$classification" == target ]]; then
    database=$(env_value "$direct_target_env" WEATHER_DATABASE_NAME)
    authorization=$(migration_authorization "$direct_target_release")
    if direct_validate_compatibility "$direct_compatibility_proof" &&
      validate_migration_authorization "$authorization" \
        "$direct_source_release" "$direct_target_release" &&
      apply_runtime_database_acl "$direct_target_env" "$database" &&
      verify_runtime_database_acl "$direct_target_env" "$database" &&
      with_restore_migration_authorization "$authorization" "$direct_source_release" \
        "$direct_target_release" start_v14_compatibility_bridge_runtime "$direct_source_env"; then
      direct_journal recovered_compatible_source absent "${DIRECT_MIGRATION_EXIT:-unknown}"
      write_private_state "$state_dir/schema-release" "$direct_target_release"
      write_private_state "$state_dir/previous-release" "$direct_source_release"
      write_active_symlink "$direct_source_release"
      write_private_state "$state_dir/current-release" "$direct_source_release"
      printf 'Direct rollout restored the compatibility-proven source runtime on schema 23.\n' >&2
      outcome=0
    else
      printf 'error: committed schema recovery lacks exact compatibility authority; preserving evidence\n' >&2
    fi
  else
    printf 'error: unknown direct migration ledger; writers remain stopped and evidence is preserved\n' >&2
  fi
  set -e
  return "$outcome"
}

# recover on process exit after the writer-stop boundary
direct_exit_recovery() {
  local status=$?
  # avoid recovery after successful marker publication
  if [[ "$direct_mutation" == true && "$direct_complete" != true ]]; then
    direct_recover_inner || :
  fi
  return "$status"
}

# activate the fixed direct migration after all external gates
direct_activate() {
  local identity=$1
  local identity_sha=$2
  local proof=$3
  local authorization=$4
  local authorization_sha=$5
  local backup backup_sha database api_writer worker_writer classification journal
  direct_begin
  direct_validate_identity "$identity" "$identity_sha"
  [[ "$proof" == "$releases_dir/$direct_target_release.direct-compatibility.json" ]] ||
    die "direct compatibility proof path differs"
  direct_validate_compatibility "$proof"
  direct_compatibility_sha=$(sha256sum "$proof" | awk '{print $1}')
  direct_source_env=$(direct_require_source)
  direct_target_env=$(direct_target_environment)
  direct_require_target "$direct_target_env" "$identity"
  validate_migration_authorization "$(migration_authorization "$direct_target_release")" \
    "$direct_source_release" "$direct_target_release"
  backup=$(direct_backup_receipt)
  direct_validate_backup "$backup" "$identity_sha"
  backup_sha=$(sha256sum "$backup" | awk '{print $1}')
  direct_backup_sha=$backup_sha
  direct_validate_activation "$authorization" "$authorization_sha" "$identity_sha" "$backup_sha"
  require_normal_release_resource_gate
  direct_require_free /var/lib/weather 2147483648
  require_deployment_secrets
  database=$(env_value "$direct_source_env" WEATHER_DATABASE_NAME)
  require_v14_compatibility_bridge_migration_ledger "$direct_source_env" "$database"
  direct_source_history=$(migration_history_sha256 "$direct_source_env" "$database")
  [[ "$direct_source_history" =~ ^[a-f0-9]{64}$ &&
    "$direct_source_history" != "$direct_target_history" ]] || die "source ledger differs"
  journal="$(direct_state_root)/activation-journal.json"
  [[ ! -e "$journal" && ! -L "$journal" ]] ||
    die "direct rollout activation journal already exists; use recover"
  api_writer=$(capture_supported_writer_container "$direct_source_env" api)
  worker_writer=$(capture_supported_writer_container "$direct_source_env" worker)
  direct_identity_sha=$identity_sha
  direct_compatibility_proof=$proof
  direct_journal prepared absent absent
  direct_mutation=true
  trap direct_exit_recovery EXIT
  stop_supported_writer_container "$direct_source_env" api "$api_writer" ||
    die "direct API writer stop failed"
  stop_supported_writer_container "$direct_source_env" worker "$worker_writer" ||
    die "direct worker stop failed"
  require_supported_writers_quiesced "$direct_source_env"
  start_postgres "$direct_target_env"
  require_supported_writers_quiesced "$direct_target_env"
  direct_require_database_quiesced "$direct_target_env"
  direct_run_migration "$direct_target_env"
  # classify only after migration removal and exact writer quiescence
  classification=$(direct_classify "$direct_target_env" "$direct_source_history")
  [[ "$classification" == target ]] || {
    [[ "$classification" == source ]] && die "direct atomic migration rolled back"
    die "direct migration ledger is unknown"
  }
  [[ "$DIRECT_MIGRATION_EXIT" == 0 ]] ||
    die "direct migration container failed after committing the target ledger"
  direct_journal committed absent "$DIRECT_MIGRATION_EXIT"
  direct_finish_target "$identity" "$direct_target_env"
  direct_complete=true
  trap - EXIT
  printf 'Activated fixed direct rollout %s.\n' "$direct_target_release"
}

# resume only the preserved exact recovery journal
direct_recover() {
  local identity=$1
  local identity_sha=$2
  local proof=$3
  local database backup journal
  direct_begin
  direct_validate_identity "$identity" "$identity_sha"
  direct_validate_compatibility "$proof"
  [[ "$proof" == "$releases_dir/$direct_target_release.direct-compatibility.json" ]] ||
    die "direct compatibility proof path differs"
  direct_compatibility_sha=$(sha256sum "$proof" | awk '{print $1}')
  backup=$(direct_backup_receipt)
  direct_validate_backup "$backup" "$identity_sha"
  direct_backup_sha=$(sha256sum "$backup" | awk '{print $1}')
  journal="$(direct_state_root)/activation-journal.json"
  direct_validate_journal "$journal" "$identity_sha" "$direct_compatibility_sha" \
    "$direct_backup_sha"
  direct_identity_sha=$identity_sha
  direct_compatibility_proof=$proof
  direct_source_env=$(release_env "$direct_source_release")
  direct_target_env=$(direct_target_environment)
  database=$(env_value "$direct_source_env" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  direct_source_history=$(direct_json "$journal" sourceHistorySha256)
  direct_recover_inner
}

# dispatch one exact phase
direct_main() {
  (($# >= 1)) || { direct_usage >&2; return 2; }
  local action=$1
  shift
  # reject all caller-selected deployment variants
  case "$action" in
    prepare)
      (($# == 2)) || { printf 'error: prepare requires two arguments\n' >&2; return 2; }
      direct_prepare "$@"
      ;;
    compatibility)
      (($# == 3)) || { printf 'error: compatibility requires three arguments\n' >&2; return 2; }
      direct_compatibility "$@"
      ;;
    backup)
      (($# == 2)) || { printf 'error: backup requires two arguments\n' >&2; return 2; }
      direct_backup "$@"
      ;;
    activate)
      (($# == 5)) || { printf 'error: activate requires five arguments\n' >&2; return 2; }
      direct_activate "$@"
      ;;
    recover)
      (($# == 3)) || { printf 'error: recover requires three arguments\n' >&2; return 2; }
      direct_recover "$@"
      ;;
    --help|-h) direct_usage ;;
    *) printf 'error: unknown direct rollout phase: %s\n' "$action" >&2; return 2 ;;
  esac
}

# execute only as the entrypoint
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  direct_main "$@"
fi
