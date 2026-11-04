#!/usr/bin/env bash
set -euo pipefail

spi_expected_update_sha=ea1185e04e0da2439c856b5b6989ce4f87f087a63ff7bafa1781c577cf42b8a0
spi_expected_control_sha=6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb
spi_expected_base_sha=12e33019380f77215a44340e4d6feb4a0dc13f76c968ab36c4501d039ec24de8
spi_release=2026.11.03-3
spi_source_env_sha=7727f844ca07fdd7fbd7a717976628d5e4f860285d5fe527e990f13632eea72b
spi_unit=weather-compose.service
spi_dropin_name=10-schema-preserving-release.conf
spi_dropin_created=false
spi_dropin_previous=

# report one bounded installer failure
spi_fail() {
  printf 'error: %s\n' "$*" >&2
  return 1
}

# hash one regular file
spi_sha() {
  sha256sum -- "$1" | awk '{print $1}'
}

# require one canonical owner-controlled directory
spi_require_directory() {
  local path=$1 privacy=${2:-public} mode
  [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" &&
    "$(stat -c '%u:%g' -- "$path")" == "$EUID:$(id -g)" ]] ||
    spi_fail "directory identity differs: $path" || return 1
  mode=$(stat -c '%a' -- "$path")
  # require a private staging root or a non-writable published root
  if [[ "$privacy" == private ]]; then
    [[ "$mode" == 700 ]] || spi_fail "directory is not private: $path"
  else
    (( (8#$mode & 022) == 0 )) || spi_fail "directory is externally writable: $path"
  fi
}

# emit the closed deterministic two-file manifest
spi_bundle_manifest() {
  local source=$1 privacy=${2:-private} name path metadata
  spi_require_directory "$source" "$privacy" || return 1
  [[ "$(find -P "$source" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)" == \
    $'schema-preserving-release.mjs\nschema-preserving-release.sh' ]] ||
    spi_fail "bundle must contain exactly the two reviewed helpers" || return 1
  # bind exact bytes, ownership, link count and modes
  for name in schema-preserving-release.mjs schema-preserving-release.sh; do
    path="$source/$name"
    metadata=$(stat -c '%u:%g:%a:%h' -- "$path" 2>/dev/null) || return 1
    [[ -f "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" &&
      "$metadata" == "$EUID:$(id -g):$([[ "$name" == *.sh ]] && printf 755 || printf 644):1" ]] ||
      spi_fail "bundle member identity differs: $name" || return 1
    printf '%s  %s\n' "$(spi_sha "$path")" "$name"
  done
}

# calculate the content-addressed bundle identity
spi_bundle_sha() {
  spi_bundle_manifest "$1" "${2:-private}" | sha256sum | awk '{print $1}'
}

# verify one complete installed bundle
spi_verify_installed_bundle() {
  local source=$1 target=$2 name metadata
  spi_require_directory "$target" public || return 1
  [[ "$(stat -c '%a' -- "$target")" == 700 || "$(stat -c '%a' -- "$target")" == 755 ]] ||
    spi_fail "installed bundle mode differs" || return 1
  [[ "$(find -P "$target" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)" == \
    $'schema-preserving-release.mjs\nschema-preserving-release.sh' ]] ||
    spi_fail "installed bundle contents differ" || return 1
  # compare every published member to its staged source
  for name in schema-preserving-release.mjs schema-preserving-release.sh; do
    metadata=$(stat -c '%u:%g:%a:%h' -- "$target/$name" 2>/dev/null) || return 1
    [[ -f "$target/$name" && ! -L "$target/$name" &&
      "$metadata" == "$EUID:$(id -g):$([[ "$name" == *.sh ]] && printf 755 || printf 644):1" &&
      "$(spi_sha "$target/$name")" == "$(spi_sha "$source/$name")" ]] ||
      spi_fail "installed bundle member differs: $name" || return 1
  done
}

# publish one immutable bundle by same-filesystem rename
spi_publish_bundle() {
  local source=$1 operators=$2 sha=$3 target="$2/$3" temporary parent
  parent=$(dirname "$operators")
  # create only the fixed operator roots below a safe Weather root
  if [[ ! -e "$parent" && ! -L "$parent" ]]; then
    spi_require_directory "$(dirname "$parent")" public || return 1
    install -d -m 0755 -o "$EUID" -g "$(id -g)" -- "$parent" || return 1
  fi
  spi_require_directory "$parent" public || return 1
  if [[ ! -e "$operators" && ! -L "$operators" ]]; then
    install -d -m 0755 -o "$EUID" -g "$(id -g)" -- "$operators" || return 1
  fi
  spi_require_directory "$operators" public || return 1
  # accept only a byte-identical complete prior publication
  if [[ -e "$target" || -L "$target" ]]; then
    spi_verify_installed_bundle "$source" "$target"
    return
  fi
  temporary=$(mktemp -d "$operators/.${sha}.XXXXXX") || return 1
  # remove only this invocation's unpublished temporary on copy failure
  if ! install -m 0644 -o "$EUID" -g "$(id -g)" -- "$source/schema-preserving-release.mjs" "$temporary/" ||
    ! install -m 0755 -o "$EUID" -g "$(id -g)" -- "$source/schema-preserving-release.sh" "$temporary/" ||
    ! sync -f "$temporary/schema-preserving-release.mjs" ||
    ! sync -f "$temporary/schema-preserving-release.sh" || ! sync -f "$temporary"; then
    rm -rf -- "$temporary"
    return 1
  fi
  chmod 0755 "$temporary" || { rm -rf -- "$temporary"; return 1; }
  mv -T -- "$temporary" "$target" || { rm -rf -- "$temporary"; return 1; }
  sync -f "$operators" || return 1
  spi_verify_installed_bundle "$source" "$target"
}

# calculate the installed control identity without executing it
spi_control_digest() {
  local deploy=$1 root file
  local -a files
  root=$(dirname "$deploy")
  mapfile -d '' -t files < <(
    find "$deploy/scripts" "$deploy/postgres" "$deploy/systemd" "$deploy/sudoers" \
      -type f -print0 | LC_ALL=C sort -z
  )
  files+=("$deploy/compose.yaml")
  # preserve the reviewed relative-path digest algorithm
  for file in "${files[@]}"; do
    printf '%s\0' "${file#"$root/"}"
    spi_sha "$file"
  done | sha256sum | awk '{print $1}'
}

# load only the reviewed installed release library
spi_load_control() {
  local deploy=$1 updater="$1/scripts/update.sh" metadata
  metadata=$(stat -c '%u:%g:%a:%h' -- "$updater" 2>/dev/null) || return 1
  [[ -f "$updater" && ! -L "$updater" && "$metadata" == "$EUID:$(id -g):755:1" &&
    "$(spi_sha "$updater")" == "$spi_expected_update_sha" &&
    "$(spi_control_digest "$deploy")" == "$spi_expected_control_sha" ]] ||
    spi_fail "installed updater identity differs" || return 1
  # source only after the immutable byte check
  # shellcheck disable=SC1090
  source "$updater"
  # shellcheck disable=SC2154
  [[ "$deploy_dir" == "$deploy" && "$state_dir" == "$deploy/state" &&
    "$releases_dir" == "$deploy/releases" && "$control_plane_version" == 17 &&
    "$(control_plane_digest)" == "$spi_expected_control_sha" ]] ||
    spi_fail "installed control-plane identity differs"
}

# require the exact active release and five healthy services
spi_require_seed() {
  local source service container health marker
  # bind both state markers to root-owned single-link files
  for marker in current-release schema-release; do
    [[ "$(stat -c '%u:%g:%a:%h' "$state_dir/$marker" 2>/dev/null)" == "$EUID:$(id -g):600:1" ]] ||
      spi_fail "release marker identity differs: $marker" || return 1
  done
  [[ "$(read_release_state "$state_dir/current-release")" == "$spi_release" &&
    "$(read_release_state "$state_dir/schema-release")" == "$spi_release" &&
    -L "$state_dir/active.env" &&
    "$(stat -c '%u:%g' "$state_dir/active.env")" == "$EUID:$(id -g)" &&
    "$(readlink "$state_dir/active.env")" == "../releases/$spi_release.env" ]] ||
    spi_fail "active schema-preserving seed differs" || return 1
  source=$(normal_release_source_env) || return 1
  [[ "$source" == "$releases_dir/$spi_release.env" && -f "$source" && ! -L "$source" &&
    "$(stat -c '%u:%g:%a:%h' "$source")" == "$EUID:$(id -g):600:1" &&
    "$(spi_sha "$source")" == "$spi_source_env_sha" ]] ||
    spi_fail "active release environment identity differs" || return 1
  # require every service to be running and every defined health check healthy
  for service in postgres api worker web cloudflared; do
    container=$(WEATHER_ENV_FILE="$source" compose ps -q "$service") || return 1
    [[ "$container" =~ ^[a-f0-9]{64}$ ]] || spi_fail "service container identity differs: $service" || return 1
    health=$(docker container inspect --format \
      '{{.State.Running}}|{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$container") || return 1
    [[ "$health" == true\|healthy || "$health" == true\|none ]] ||
      spi_fail "service is not healthy: $service" || return 1
  done
}

# reduce one effective command to its path and argv vector
spi_effective_vector() {
  local property=$1
  systemctl show "$spi_unit" --property="$property" --value | python3 -c '
import re,sys
value=sys.stdin.read().strip()
matches=re.findall(r"\{ path=([^ ;]+) ; argv\[\]=([^;]+?) ;", value)
if len(matches)!=1: raise SystemExit(1)
print(matches[0][0]+"|"+matches[0][1].strip())'
}

# verify and hash the unchanged base unit
spi_base_unit_sha() {
  local deploy=$1 units=$2 base="$2/$spi_unit" source="$1/systemd/$spi_unit"
  spi_require_directory "$units" public || return 1
  [[ -f "$base" && ! -L "$base" && "$(stat -c '%u:%g:%a:%h' "$base")" == "$EUID:$(id -g):644:1" &&
    -f "$source" && ! -L "$source" && "$(realpath -e "$source")" == "$source" &&
    "$(stat -c '%u:%g:%h' "$source")" == "$EUID:$(id -g):1" &&
    "$(spi_sha "$base")" == "$spi_expected_base_sha" &&
    "$(spi_sha "$base")" == "$(spi_sha "$source")" ]] ||
    spi_fail "base systemd unit identity differs: $(stat -c '%u:%g:%a:%h' "$base" 2>/dev/null || true) / $(stat -c '%u:%g:%a:%h' "$source" 2>/dev/null || true)" || return 1
  spi_sha "$base"
}

# reject foreign drop-ins and publish the exact override
spi_publish_dropin() {
  local units=$1 desired=$2 directory="$1/$spi_unit.d" target="$1/$spi_unit.d/$spi_dropin_name" entry temporary
  local predecessor=${3:-}
  if [[ -e "$directory" || -L "$directory" ]]; then
    spi_require_directory "$directory" public || return 1
    # reject every unrelated or non-regular drop-in entry
    while IFS= read -r entry; do
      [[ "$entry" == "$target" && -f "$entry" && ! -L "$entry" ]] ||
        spi_fail "unknown systemd drop-in exists: $entry" || return 1
    done < <(find -P "$directory" -mindepth 1 -maxdepth 1 -print)
  else
    install -d -m 0755 -o "$EUID" -g "$(id -g)" -- "$directory" || return 1
    sync -f "$units" || return 1
  fi
  # retain a pre-existing byte-identical drop-in without claiming ownership
  if [[ -e "$target" || -L "$target" ]]; then
    [[ -f "$target" && ! -L "$target" && "$(stat -c '%u:%g:%a:%h' "$target")" == "$EUID:$(id -g):644:1" ]] ||
      spi_fail "existing systemd drop-in metadata differs" || return 1
    # keep a byte-identical installation idempotent
    if [[ "$(spi_sha "$target")" == "$(printf '%s' "$desired" | sha256sum | awk '{print $1}')" ]]; then
      return
    fi
    [[ "$predecessor" =~ ^[a-f0-9]{64}$ && "$(spi_sha "$target")" == "$predecessor" ]] ||
      spi_fail "existing systemd drop-in differs" || return 1
    spi_dropin_previous="$(cat "$target")"$'\n'
  fi
  temporary=$(mktemp "$directory/.${spi_dropin_name}.XXXXXX") || return 1
  # remove only this invocation's unpublished temporary on write failure
  if ! printf '%s' "$desired" >"$temporary" || ! chmod 0644 "$temporary" ||
    ! sync -f "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  mv -T -- "$temporary" "$target" || { rm -f -- "$temporary"; return 1; }
  spi_dropin_created=true
  sync -f "$directory" || return 1
  sync -f "$units" || return 1
}

# render one deterministic create-once installation receipt
spi_receipt_bytes() {
  python3 - "$@" <<'PY'
import json,sys
bundle,base,dropin,helper,start,reload,stop=sys.argv[1:]
value={"baseUnitSha256":base,"bundleSha256":bundle,
"contractVersion":"weather-schema-preserving-release-installation/v1",
"controlPlaneSha256":"6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
"controlPlaneVersion":17,"dropInSha256":dropin,"execReload":reload,
"execStart":start,"execStop":stop,"helperDirectory":helper,
"helperScript":helper+"/schema-preserving-release.sh","timeoutStartSec":600}
print(json.dumps(value,sort_keys=True,separators=(",",":")))
PY
}

# publish a root-private receipt without replacing prior evidence
spi_publish_receipt() {
  local deploy=$1 bytes=$2 directory="$1/state/schema-preserving-release-v1" target temporary
  target="$directory/installation.json"
  if [[ ! -e "$directory" && ! -L "$directory" ]]; then
    install -d -m 0700 -o "$EUID" -g "$(id -g)" -- "$directory" || return 1
  fi
  spi_require_directory "$directory" private || return 1
  # publish each replacement authority without overwriting its predecessor's receipt
  if [[ -n "${3:-}" ]]; then
    [[ "$3" =~ ^[a-f0-9]{64}$ ]] || spi_fail "receipt bundle identity differs" || return 1
    directory="$directory/installations"
    # create only the private immutable receipt collection
    if [[ ! -e "$directory" && ! -L "$directory" ]]; then
      install -d -m 0700 -o "$EUID" -g "$(id -g)" -- "$directory" || return 1
    fi
    spi_require_directory "$directory" private || return 1
    target="$directory/$3.json"
  fi
  # permit only an identical immutable retry
  if [[ -e "$target" || -L "$target" ]]; then
    [[ -f "$target" && ! -L "$target" && "$(stat -c '%u:%g:%a:%h' "$target")" == "$EUID:$(id -g):600:1" &&
      "$(cat "$target")" == "${bytes%$'\n'}" ]] || spi_fail "installation receipt differs"
    return
  fi
  temporary=$(mktemp "$directory/.installation.XXXXXX") || return 1
  # remove only this invocation's unpublished temporary on write failure
  if ! printf '%s\n' "$bytes" >"$temporary" || ! chmod 0600 "$temporary" ||
    ! sync -f "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  ln -- "$temporary" "$target" || { rm -f -- "$temporary"; return 1; }
  # retract both invocation-owned links if receipt durability fails
  if ! rm -f -- "$temporary" || ! sync -f "$directory"; then
    rm -f -- "$temporary" "$target"
    sync -f "$directory" || true
    return 1
  fi
}

# reload and prove the effective command vectors
spi_apply_systemd() {
  local desired_start=$1 before_stop=$2
  systemctl daemon-reload || return 1
  systemd-analyze verify "$spi_unit" || return 1
  [[ "$(spi_effective_vector ExecStart)" == "$desired_start" &&
    "$(spi_effective_vector ExecReload)" == "$desired_start" &&
    "$(spi_effective_vector ExecStop)" == "$before_stop" &&
    "$(systemctl show "$spi_unit" --property=TimeoutStartUSec --value)" == 10min ]]
}

# restore only a drop-in created by this invocation
spi_rollback_dropin() {
  local target=$1 before_start=$2 before_reload=$3 before_stop=$4 before_timeout=$5 temporary
  # leave all pre-existing configuration untouched
  if [[ "$spi_dropin_created" == true ]]; then
    # restore an explicitly verified predecessor instead of deleting its configuration
    if [[ -n "$spi_dropin_previous" ]]; then
      temporary=$(mktemp "${target%/*}/.restore.XXXXXX") || return 1
      if ! printf '%s' "$spi_dropin_previous" >"$temporary" || ! chmod 0644 "$temporary" ||
        ! sync -f "$temporary" || ! mv -T -- "$temporary" "$target"; then
        rm -f -- "$temporary"
        return 1
      fi
      sync -f "${target%/*}" || return 1
    else
      rm -f -- "$target" || return 1
    fi
    systemctl daemon-reload || return 1
    [[ "$(spi_effective_vector ExecStart)" == "$before_start" &&
      "$(spi_effective_vector ExecReload)" == "$before_reload" &&
      "$(spi_effective_vector ExecStop)" == "$before_stop" &&
      "$(systemctl show "$spi_unit" --property=TimeoutStartUSec --value)" == "$before_timeout" ]]
  fi
}

# perform the locked content-addressed installation
spi_install() {
  local source=$1 expected=$2 deploy=$3 operators=$4 units=$5 actual base_sha helper desired dropin_sha
  local before_start before_reload before_stop before_timeout desired_vector receipt target
  actual=$(spi_bundle_sha "$source") || return 1
  [[ "$expected" =~ ^[a-f0-9]{64}$ && "$actual" == "$expected" ]] ||
    spi_fail "bundle SHA-256 differs" || return 1
  spi_load_control "$deploy" || return 1
  acquire_release_transaction_lock || return 1
  spi_require_seed || return 1
  base_sha=$(spi_base_unit_sha "$deploy" "$units") || return 1
  before_start=$(spi_effective_vector ExecStart) || return 1
  before_reload=$(spi_effective_vector ExecReload) || return 1
  before_stop=$(spi_effective_vector ExecStop) || return 1
  before_timeout=$(systemctl show "$spi_unit" --property=TimeoutStartUSec --value) || return 1
  spi_publish_bundle "$source" "$operators" "$expected" || return 1
  helper="$operators/$expected"
  desired="[Service]
ExecStart=
ExecStart=$helper/schema-preserving-release.sh system-recover
ExecReload=
ExecReload=$helper/schema-preserving-release.sh system-recover
TimeoutStartSec=600
"
  dropin_sha=$(printf '%s' "$desired" | sha256sum | awk '{print $1}')
  desired_vector="$helper/schema-preserving-release.sh|$helper/schema-preserving-release.sh system-recover"
  receipt=$(spi_receipt_bytes "$expected" "$base_sha" "$dropin_sha" "$helper" \
    "$desired_vector" "$desired_vector" "$before_stop") || return 1
  target="$units/$spi_unit.d/$spi_dropin_name"
  spi_dropin_created=false
  spi_dropin_previous=
  # publish inert recovery authority before systemd can expose the helper
  spi_publish_receipt "$deploy" "$receipt" || return 1
  if ! spi_publish_dropin "$units" "$desired" ||
    ! spi_apply_systemd "$desired_vector" "$before_stop" ||
    # revalidate the exact prepublished receipt after effective-vector proof
    ! spi_publish_receipt "$deploy" "$receipt"; then
    spi_rollback_dropin "$target" "$before_start" "$before_reload" "$before_stop" "$before_timeout" ||
      spi_fail "installation failed and prior command vectors were not restored"
    return 1
  fi
}

# render the sole boot entrypoint for one immutable bundle
spi_dropin_bytes() {
  printf '[Service]\nExecStart=\nExecStart=%s/schema-preserving-release.sh system-recover\nExecReload=\nExecReload=%s/schema-preserving-release.sh system-recover\nTimeoutStartSec=600\n' "$1" "$1"
}

# verify a complete private receipt without executing its contents
spi_require_receipt() {
  local path=$1 expected=$2
  [[ -f "$path" && ! -L "$path" && "$(stat -c '%u:%g:%a:%h' "$path")" == "$EUID:$(id -g):600:1" &&
    "$(spi_sha "$path")" == "$(printf '%s\n' "$expected" | sha256sum | awk '{print $1}')" ]] ||
    spi_fail "predecessor installation receipt differs"
}

# reuse audited read-only runtime checks while the installer retains the release lock
spi_runtime_check() {
  # shellcheck disable=SC2154
  bash -c '
set -euo pipefail
source "$1/schema-preserving-release.sh"
[[ "$(sp_control_digest "${sp_update%/scripts/update.sh}")" == "$sp_control_sha" ]]
source "$sp_update"
[[ "$5" =~ ^[0-9]+$ && "$(stat -Lc "%d:%i" "/proc/self/fd/$5")" == "$(stat -c "%d:%i" "$state_dir")" ]]
[[ "$4" == snapshot || "$4" == unchanged ]]
flock --exclusive --nonblock "$5"
require_no_dashboard_override
[[ ! -e "$sp_journal" && ! -L "$sp_journal" ]]
# retain internal and final source failures instead of trusting stdout alone
source_environment=$(set -e; sp_source)
[[ "$source_environment" == "$2" ]]
sp_health "$2"
sp_database "$2"
# compare only the immutable metadata and retained infrastructure snapshot
if [[ "$4" == snapshot ]]; then sp_snapshot "$2" "$3"; else sp_unchanged "$2" "$3"; fi
' schema-preserving-handoff "$1" "$2" "$3" "$4" "$release_transaction_lock_fd"
}

# replace only one explicitly named predecessor while preserving both boot authorities
spi_upgrade() {
  local source=$1 expected=$2 previous=$3 deploy=$4 operators=$5 units=$6 actual old_actual current environment
  local root receipt prior_receipt base_sha old_helper helper old_desired desired old_sha dropin_sha target
  local before_start before_reload before_stop before_timeout old_vector desired_vector prior_bytes snapshot disk_sha marker
  [[ "$expected" =~ ^[a-f0-9]{64}$ && "$previous" =~ ^[a-f0-9]{64}$ && "$expected" != "$previous" ]] ||
    spi_fail "upgrade requires distinct exact old and new bundle identities" || return 1
  actual=$(spi_bundle_sha "$source") || return 1
  [[ "$actual" == "$expected" ]] || spi_fail "bundle SHA-256 differs" || return 1
  spi_load_control "$deploy" || return 1
  acquire_release_transaction_lock || return 1
  root="$deploy/state/schema-preserving-release-v1"
  spi_require_directory "$root" private || return 1
  [[ ! -e "$root/journal.json" && ! -L "$root/journal.json" ]] || spi_fail "release journal requires recovery" || return 1
  # bind both authority markers before accepting an application descendant
  for marker in current-release schema-release; do
    [[ -f "$state_dir/$marker" && ! -L "$state_dir/$marker" &&
      "$(stat -c '%u:%g:%a:%h' "$state_dir/$marker")" == "$EUID:$(id -g):600:1" ]] ||
      spi_fail "release marker identity differs: $marker" || return 1
  done
  [[ "$(read_release_state "$state_dir/schema-release")" == "$spi_release" ]] || spi_fail "schema owner differs" || return 1
  current=$(read_release_state "$state_dir/current-release") || return 1
  [[ "$current" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]?$ ]] || spi_fail "current release differs" || return 1
  environment="$deploy/releases/$current.env"
  [[ -f "$environment" && ! -L "$environment" && "$(stat -c '%u:%g:%a:%h' "$environment")" == "$EUID:$(id -g):600:1" &&
    -L "$state_dir/active.env" && "$(readlink "$state_dir/active.env")" == "../releases/$current.env" ]] ||
    spi_fail "active application environment differs" || return 1
  old_helper="$operators/$previous"
  old_actual=$(spi_bundle_sha "$old_helper" public) || return 1
  [[ "$old_actual" == "$previous" ]] || spi_fail "predecessor bundle differs" || return 1
  base_sha=$(spi_base_unit_sha "$deploy" "$units") || return 1
  before_start=$(spi_effective_vector ExecStart) || return 1
  before_reload=$(spi_effective_vector ExecReload) || return 1
  before_stop=$(spi_effective_vector ExecStop) || return 1
  before_timeout=$(systemctl show "$spi_unit" --property=TimeoutStartUSec --value) || return 1
  old_vector="$old_helper/schema-preserving-release.sh|$old_helper/schema-preserving-release.sh system-recover"
  old_desired="$(spi_dropin_bytes "$old_helper")"$'\n'
  old_sha=$(printf '%s' "$old_desired" | sha256sum | awk '{print $1}')
  prior_receipt="$root/installations/$previous.json"
  # retain compatibility with the immutable original installation receipt
  if [[ ! -e "$prior_receipt" && ! -L "$prior_receipt" ]]; then prior_receipt="$root/installation.json"; fi
  prior_bytes=$(spi_receipt_bytes "$previous" "$base_sha" "$old_sha" "$old_helper" "$old_vector" "$old_vector" "$before_stop") || return 1
  spi_require_receipt "$prior_receipt" "$prior_bytes" || return 1
  helper="$operators/$expected"
  desired="$(spi_dropin_bytes "$helper")"$'\n'
  dropin_sha=$(printf '%s' "$desired" | sha256sum | awk '{print $1}')
  desired_vector="$helper/schema-preserving-release.sh|$helper/schema-preserving-release.sh system-recover"
  receipt=$(spi_receipt_bytes "$expected" "$base_sha" "$dropin_sha" "$helper" "$desired_vector" "$desired_vector" "$before_stop") || return 1
  target="$units/$spi_unit.d/$spi_dropin_name"
  [[ -f "$target" && ! -L "$target" && "$(stat -c '%u:%g:%a:%h' "$target")" == "$EUID:$(id -g):644:1" && "$before_timeout" == 10min ]] ||
    spi_fail "predecessor systemd authority differs" || return 1
  disk_sha=$(spi_sha "$target")
  [[ ( "$disk_sha" == "$old_sha" && "$before_start" == "$old_vector" && "$before_reload" == "$old_vector" ) ||
    ( "$disk_sha" == "$dropin_sha" && ( "$before_start" == "$old_vector" || "$before_start" == "$desired_vector" ) && "$before_reload" == "$before_start" ) ]] ||
    spi_fail "effective predecessor vectors differ" || return 1
  # an interrupted switch is accepted only with its already published target receipt
  if [[ "$disk_sha" == "$dropin_sha" ]]; then spi_require_receipt "$root/installations/$expected.json" "$receipt" || return 1; fi
  spi_publish_bundle "$source" "$operators" "$expected" || return 1
  snapshot=$(mktemp "$root/.handoff-snapshot.XXXXXX") || return 1
  rm -- "$snapshot" || return 1
  spi_runtime_check "$helper" "$environment" "$snapshot" snapshot || return 1
  spi_publish_receipt "$deploy" "$receipt" "$expected" || return 1
  # a fully verified completed retry needs no systemd mutation
  if [[ "$disk_sha" == "$dropin_sha" && "$before_start" == "$desired_vector" ]]; then return 0; fi
  spi_dropin_previous="$old_desired"
  spi_dropin_created=false
  # finish only the exact on-disk switch left by the same requested handoff
  if [[ "$disk_sha" == "$dropin_sha" ]]; then spi_dropin_created=true; fi
  if ! spi_publish_dropin "$units" "$desired" "$old_sha" ||
    ! spi_apply_systemd "$desired_vector" "$before_stop" ||
    ! spi_runtime_check "$helper" "$environment" "$snapshot" unchanged ||
    ! spi_require_receipt "$root/installations/$expected.json" "$receipt"; then
    spi_rollback_dropin "$target" "$old_vector" "$old_vector" "$before_stop" "$before_timeout" ||
      spi_fail "handoff failed and predecessor vectors were not restored"
    return 1
  fi
}

# run only when invoked directly
main() {
  [[ $# == 2 || ( $# == 4 && "$1" == --upgrade ) ]] || {
    printf 'Usage: %s BUNDLE_DIRECTORY EXPECTED_BUNDLE_SHA256 | --upgrade BUNDLE_DIRECTORY NEW_SHA256 OLD_SHA256\n' "$0" >&2; return 2;
  }
  [[ "$EUID" == 0 ]] || spi_fail "installer requires root" || return 1
  # bootstrap and replacement keep separate authority gates
  if [[ "$1" == --upgrade ]]; then
    spi_upgrade "$2" "$3" "$4" /opt/weather/current/deploy \
      /opt/weather/operators/schema-preserving-release /etc/systemd/system
  else
    spi_install "$1" "$2" /opt/weather/current/deploy \
      /opt/weather/operators/schema-preserving-release /etc/systemd/system
  fi
}

# keep the file safe to source in focused tests
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
