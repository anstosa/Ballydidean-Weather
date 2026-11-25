#!/usr/bin/env bash
set -euo pipefail

# keep host application artifacts outside both runtime images and the immutable base control tree
readonly native_evidence_root=/opt/weather/operators/native-evidence
readonly native_legacy_dispatch="$native_evidence_root/legacy-remote-ops.sh"
readonly native_gateway=/usr/local/sbin/weather-remote-ops

# report the stable remote operation failure channel
native_gateway_fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

# require one exact installed regular root-owned file
native_gateway_file() {
  local path=$1 mode=$2
  [[ -f "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" &&
    "$(stat -c '%u:%g:%a:%h' -- "$path")" == "0:0:$mode:1" ]] ||
    native_gateway_fail "installed native evidence file differs"
}

# derive and verify the actual immutable package manifest
native_gateway_package() {
  local commit=$1 pointer bundle member mode manifest
  [[ "$commit" =~ ^[a-f0-9]{40}$ ]] || native_gateway_fail "native evidence source commit differs"
  pointer="$native_evidence_root/by-commit/$commit"
  native_gateway_file "$pointer" 600
  bundle=$(cat "$pointer")
  [[ "$bundle" =~ ^[a-f0-9]{64}$ ]] || native_gateway_fail "native evidence package identity differs"
  native_evidence_package="$native_evidence_root/packages/$bundle"
  [[ -d "$native_evidence_package" && ! -L "$native_evidence_package" &&
    "$(realpath -e -- "$native_evidence_package")" == "$native_evidence_package" &&
    "$(stat -c '%u:%g:%a' "$native_evidence_package")" == 0:0:755 ]] ||
    native_gateway_fail "native evidence package directory differs"
  manifest=
  # preserve bytewise order and verify each exact member before use
  for member in native-dashboard-release-control.sh native-evidence-remote-dispatch.sh native-generation-remote-ops.sh native-release-control-remote-dispatch.sh source-commit weather-evidence; do
    mode=644
    [[ "$member" != weather-evidence && "$member" != native-generation-remote-ops.sh ]] || mode=755
    native_gateway_file "$native_evidence_package/$member" "$mode"
    manifest+="$(sha256sum -- "$native_evidence_package/$member" | awk '{print $1}')  $member"$'\n'
  done
  native_gateway_file "$native_evidence_package/manifest.sha256" 644
  [[ "$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')" == "$bundle" &&
    "$(sha256sum "$native_evidence_package/manifest.sha256" | awk '{print $1}')" == "$bundle" &&
    "$(cat "$native_evidence_package/source-commit")" == "$commit" ]] ||
    native_gateway_fail "native evidence package checksum differs"
  # the sourced native dispatcher consumes this exact executable path
  # shellcheck disable=SC2034
  native_evidence_binary="$native_evidence_package/weather-evidence"
}

# load the actual installed source authority under the existing release transaction lock
native_gateway_context() {
  local unit helper bundle manifest member expected environment generation release publication
  unit=$(systemctl show weather-compose.service --property=ExecStart --value)
  helper=$(python3 -c 'import re,sys
s=sys.stdin.read()
m=re.fullmatch(r"\{ path=(/opt/weather/operators/schema-preserving-release/([a-f0-9]{64})/schema-preserving-release\.sh) ; argv\[\]=\1 system-recover ;[^}]+\}\s*",s)
if not m: raise SystemExit(1)
print(m[1])' <<<"$unit") || native_gateway_fail "effective application recovery helper differs"
  bundle=$(dirname "$helper")
  expected=${bundle##*/}
  manifest=
  # authenticate the content-addressed two-file helper before sourcing its functions
  for member in schema-preserving-release.mjs schema-preserving-release.sh; do
    if [[ "$member" == *.sh ]]; then native_gateway_file "$bundle/$member" 755; else native_gateway_file "$bundle/$member" 644; fi
    manifest+="$(sha256sum -- "$bundle/$member" | awk '{print $1}')  $member"$'\n'
  done
  [[ "$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')" == "$expected" ]] ||
    native_gateway_fail "installed auxiliary recovery bundle differs"
  # shellcheck disable=SC1090
  source "$helper"
  # retain the exact lock mode selected by the closed command grammar
  if [[ ${1-} == --inherited-family-lock || ${1-} == --inherited-dashboard-lock ||
    ${1-} == --dashboard-override || ${1-} == --committed-dashboard-context || ${1-} == --system-recovery ]]; then
    sp_load "$1"
  else
    sp_load
  fi
  # exclude an incomplete application switch from dashboard-only operations
  if [[ ${1-} == --dashboard-override || ${1-} == --inherited-dashboard-lock ]] ||
    { [[ ${1-} == --committed-dashboard-context ]] && dashboard_override_present; }; then
    # the authenticated auxiliary controller defines the journal path
    # shellcheck disable=SC2154
    [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]] || native_gateway_fail "application release recovery required"
  fi
  # only the family command may inspect the committed marker of an interrupted family switch
  if [[ ${1-} == --inherited-family-lock || ${1-} == --family-recovery ]] &&
    [[ -e "$sp_journal" || -L "$sp_journal" ]]; then
    environment=$(sp_native_family_interrupted_journal) || native_gateway_fail "family recovery source unavailable"
  elif [[ ${1-} == --system-recovery && ( -e "$sp_journal" || -L "$sp_journal" ) ]]; then
    # authorize only the retained marker before the exact auxiliary controller reconciles the journal
    # shellcheck disable=SC2154
    environment=$(sp_environment "$(read_release_state "$state_dir/current-release")") ||
      native_gateway_fail "system recovery source unavailable"
  else
    environment=$(sp_source) || native_gateway_fail "current application release unavailable"
  fi
  generation=$(sp_runtime_generation "$environment") || native_gateway_fail "application generation unavailable"
  native_active_environment=$environment
  native_active_generation=$generation
  native_active_commit=
  if [[ "$generation" == node-v1 ]]; then
    return
  fi
  [[ "$generation" == rust-v1 ]] || native_gateway_fail "application generation differs"
  release=$(env_value "$environment" WEATHER_RELEASE)
  # shellcheck disable=SC2154
  publication="$sp_root/$release/publication.json"
  sp_private "$publication"
  # shellcheck disable=SC2154
  node "$sp_module" validate-publication "$publication" "$release" >/dev/null ||
    native_gateway_fail "active native publication differs"
  native_active_commit=$(sp_field "$publication" targetCommit)
  # recreate only the original root-authorized anchor control coordinates
  WEATHER_ANCHOR_CONTROL_SHA256=$(env_value "$environment" WEATHER_CONTROL_PLANE_SHA256)
  WEATHER_ANCHOR_CONTROL_VERSION=$(env_value "$environment" WEATHER_CONTROL_PLANE_VERSION)
  export WEATHER_ANCHOR_CONTROL_SHA256 WEATHER_ANCHOR_CONTROL_VERSION
}

# release the classification lock before choosing the legacy or native gateway
native_gateway_generation() (
  native_gateway_context "$@"
  printf '%s\t%s\n' "$native_active_generation" "$native_active_commit"
)

# keep the exact source environment and operator lock throughout the native operation
native_gateway_execute() (
  local expected_commit=$1
  shift
  # permit only the existing family operation to resume an interrupted family journal
  if [[ "$1" == adjustment-family-release ]]; then
    native_gateway_context --family-recovery
  elif native_gateway_dashboard_action "$1"; then
    native_gateway_context --dashboard-override
  elif native_gateway_core_action "$1"; then
    native_gateway_context
  else
    native_gateway_context --committed-dashboard-context
  fi
  [[ "$native_active_generation" == rust-v1 && "$native_active_commit" == "$expected_commit" ]] ||
    native_gateway_fail "application generation changed before native operation"
  # admit the exact override tuple for dashboard actions and routine native owners
  if native_gateway_dashboard_action "$1" ||
    { ! native_gateway_core_action "$1" && dashboard_override_present; }; then
    native_dashboard_context_admission
  fi
  # preserve the deployed public-station historical wrapper in the native generation
  if [[ "$1" == public-stations-backfill || "$1" == tide-backfill || "$1" == tempest-backfill ]]; then
    (($# == 1)) || native_gateway_fail "historical backfill takes no arguments"
    native_host_station_backfill "$1"
    return
  fi
  # route the existing application release-control commands before evidence dispatch
  if native_release_control_handles "$1"; then
    native_release_control_dispatch "$@"
  else
    native_evidence_remote_dispatch "$@"
  fi
)

# preserve each original station backfill's report identity and date arguments
native_host_station_backfill() (
  local family=$1 release report_directory report partial label
  local -a arguments
  case "$family" in
    public-stations-backfill)
      arguments=(--site ballydidean --resume)
      label=Public-station
      ;;
    tempest-backfill)
      arguments=(--site ballydidean --resume)
      label=Tempest
      ;;
    tide-backfill)
      arguments=(--site ballydidean --from 2019-01-01 --resume)
      label="NOAA tide"
      ;;
    *) native_gateway_fail "historical backfill unavailable" ;;
  esac
  acquire_weather_lifecycle_lock
  release=$(env_value "$native_active_environment" WEATHER_RELEASE)
  report_directory=/var/lib/weather
  report="$report_directory/$family-$release.json"
  mkdir -p "$report_directory"
  umask 077
  partial=$(mktemp "$report_directory/.$family-$release.XXXXXX.partial")
  trap 'rm -f -- "$partial"' EXIT
  sp_compose "$native_active_environment" run --rm --no-deps worker \
    /opt/weather/bin/weather-worker "$family" "${arguments[@]}" >"$partial"
  chmod 600 "$partial"
  mv -- "$partial" "$report"
  trap - EXIT
  printf '%s historical import completed for %s\nReport: %s\n' "$label" "$release" "$report"
)

# retain the original service owner identity for each custody phase
native_host_service() {
  setpriv --reuid=10002 --regid=10002 --clear-groups -- "$native_evidence_binary" "$@"
}

# execute one fixed owner SQL stream inside the existing database container
native_host_owner_sql() {
  local database=$1
  # keep the owner credential in the existing database-side secret mount
  # shellcheck disable=SC2016
  WEATHER_ENV_FILE="$native_active_environment" compose exec -T postgres \
    sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
    adjustment-shadow-metadata "$database"
}

# initialize only the schedule independently derived from the retained root epoch
native_host_registration_initialize() (
  local bootstrap=$1 database sql
  require_deployment_secrets
  database=$(env_value "$native_active_environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  sp_database "$native_active_environment"
  # complete the native request projection before the owner SQL transaction can start
  # shellcheck disable=SC2154
  sql=$(mktemp "$state_dir/.registration-schedule.XXXXXX.sql")
  chmod 600 "$sql"
  # remove only this invocation's bounded SQL inode
  trap 'rm -f -- "$sql"' EXIT
  "$native_evidence_binary" registration-schedule-initialize-sql-v3 "$bootstrap" >"$sql"
  native_host_owner_sql "$database" <"$sql" |
    "$native_evidence_binary" verify-registration-schedule-initialization-v3 "$bootstrap"
)

# read the exact retained registration lifecycle through the existing owner role
native_host_registration_lifecycle() {
  local database
  require_deployment_secrets
  database=$(env_value "$native_active_environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  sp_database "$native_active_environment"
  "$native_evidence_binary" registration-lifecycle-status-sql-v4 |
    native_host_owner_sql "$database" |
    "$native_evidence_binary" project-registration-lifecycle-status-v4
}

# authenticate the complete owner request before opening its fixed transaction
native_host_owner_operation() (
  local kind=$1 identity=$2 database sql result_kind
  # select the response grammar independently of the request kind
  case "$kind" in
    access) result_kind=access ;;
    terminal|unsupported-terminal) result_kind=terminal ;;
    retire|unsupported-retire) result_kind=retire ;;
    *) native_gateway_fail "native owner operation is unavailable" ;;
  esac
  require_deployment_secrets
  database=$(env_value "$native_active_environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  sp_database "$native_active_environment"
  # shellcheck disable=SC2154
  sql=$(mktemp "$state_dir/.owner-operation.XXXXXX.sql")
  chmod 600 "$sql"
  trap 'rm -f -- "$sql"' EXIT
  "$native_evidence_binary" owner-operation-sql-v3 "$kind" "$identity" >"$sql"
  native_host_owner_sql "$database" <"$sql" |
    "$native_evidence_binary" owner-operation-result-v3 "$result_kind"
)

# preserve the original confirmation owner entrypoint
native_host_owner_access() {
  native_host_owner_operation access "$1"
}

# finalize the original retained shadow metadata sequence using the native owners
native_host_shadow_finalize() {
  local proof=$1 database preparation consumed
  require_deployment_secrets
  database=$(env_value "$native_active_environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  # the Rust generation is pinned to the exact schema-preserving ledger
  sp_database "$native_active_environment"
  if consumed=$(native_host_service shadow-metadata-custody-consumed-v1 "$proof"); then
    # return the exact already consumed acknowledgement on retry
    if [[ "$consumed" != null ]]; then
      printf '%s\n' "$consumed"
      return
    fi
  fi
  preparation=$(native_host_service shadow-metadata-preparation-identity-v1 "$proof")
  # persist the native preparation before the owner finalization transaction
  if [[ "$preparation" == unprepared ]]; then
    native_host_service shadow-metadata-preparation-sql-v1 "$proof" |
      native_host_owner_sql "$database" |
      native_host_service prepare-shadow-metadata-custody-v1 "$proof" >/dev/null
    preparation=$(native_host_service shadow-metadata-preparation-identity-v1 "$proof")
  fi
  [[ "$preparation" =~ ^[a-f0-9]{64}$ ]] || native_gateway_fail "shadow metadata custody preparation is unavailable"
  native_host_service shadow-metadata-finalization-sql-v1 "$preparation" |
    native_host_owner_sql "$database" |
    native_host_service consume-shadow-metadata-custody-v1 "$preparation"
}


# retain the exact scorecard installation and recoverable pointer rotation on native generations
native_host_scorecard_install() (
  scorecard_root=${WEATHER_ADJUSTMENT_SCORECARD_ROOT:-/var/lib/weather/xweather/adjustment-evidence}
  v2_scorecard_root=/var/lib/weather/xweather/adjustment-evidence
  runtime_uid=10002
  runtime_gid=10002
  install_mode=v1

  # accept only the legacy or exact v2 internal grammar
  if (($# == 1)) && [[ "$1" =~ ^[a-f0-9]{64}$ ]]; then
    expected_sha256=$1
  elif (($# == 2)) && [[ "$1" == --v2 ]] && [[ "$2" =~ ^[a-f0-9]{64}$ ]]; then
    install_mode=v2
    expected_sha256=$2
  else
    die "usage: install-adjustment-scorecard.sh [--v2] SHA256"
  fi

  # keep the privileged v2 operation on its fixed production root
  if [[ "$install_mode" == v2 ]]; then
    scorecard_root=$v2_scorecard_root
  fi

  # require the root-owned forced operation
  if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
    die "adjustment scorecard installation must run through sudo"
  fi

  require_command flock
  require_command setpriv
  require_command sha256sum
  require_command cut
  require_command stat
  require_command sync
  [[ "$(realpath -m -- "$scorecard_root")" == "$scorecard_root" ]] ||
    die "adjustment scorecard root must use a canonical path"
  [[ ! -L "$scorecard_root" ]] || die "adjustment scorecard root is linked"
  [[ ! -L "$scorecard_root/scorecards" ]] || die "scorecard object directory is linked"
  [[ ! -L "$scorecard_root/scorecards-v2" ]] || die "v2 scorecard object directory is linked"
  install -d -o "$runtime_uid" -g "$runtime_gid" -m 0700 \
    "$scorecard_root" "$scorecard_root/scorecards"

  # serialize both generations on the fixed root inode without adding a lock file
  exec {scorecard_lock_fd}<"$scorecard_root"
  flock --exclusive --nonblock "$scorecard_lock_fd" ||
    die "another scorecard installation is in flight"

  # create the separate bounded v2 hot-object directory only for v2 installation
  if [[ "$install_mode" == v2 ]]; then
    install -d -o "$runtime_uid" -g "$runtime_gid" -m 0700 \
      "$scorecard_root/scorecards-v2"
  fi
  temporary=$(mktemp "$scorecard_root/.scorecard.XXXXXXXX.partial")
  pointer_temporary=$(mktemp "$scorecard_root/.current.XXXXXXXX.partial")
  previous_temporary=$(mktemp "$scorecard_root/.previous.XXXXXXXX.partial")

  # remove only uncommitted publication files
  # shellcheck disable=SC2329
  cleanup() {
    rm -f -- "$temporary" "$pointer_temporary" "$previous_temporary"
  }
  trap cleanup EXIT
  trap 'exit 130' HUP INT TERM

  # validate the original bytes through the immutable native evidence executable
  "$native_evidence_binary" validate-scorecard-install "$install_mode" "$expected_sha256" >"$temporary"
  sync -f "$temporary"

  object_directory="$scorecard_root/scorecards"

  # isolate v2 hot-object accounting from immutable legacy v1 history
  if [[ "$install_mode" == v2 ]]; then
    object_directory="$scorecard_root/scorecards-v2"
  fi
  object="$object_directory/sha256-$expected_sha256.json"
  require_canonical_descendant "$object" "$object_directory" "scorecard object"

  # publish or verify the immutable content object
  publish_object() {
    # reuse only the exact previously committed content object
    if [[ -e "$object" || -L "$object" ]]; then
      verify_selected_object "$object_directory" "$expected_sha256"
      rm -f -- "$temporary"
    else
      chown "$runtime_uid:$runtime_gid" "$temporary"
      chmod 0600 "$temporary"
      ln "$temporary" "$object" || die "scorecard object publication raced"
      rm -f -- "$temporary"
      sync -f "$object"
      sync -f "$object_directory"
    fi

    # prove the runtime identity can read the object before selecting it
    setpriv --reuid="$runtime_uid" --regid="$runtime_gid" --clear-groups \
      test -r "$object"
  }

  # verify one selected immutable object beneath its fixed generation directory
  verify_selected_object() {
    local directory=$1
    local sha256=$2
    local selected_object="$directory/sha256-$sha256.json"
    require_canonical_descendant "$selected_object" "$directory" "selected scorecard object"
    [[ -f "$selected_object" && ! -L "$selected_object" ]] ||
      die "selected scorecard object is invalid"
    [[ "$(stat -c '%a %h' -- "$selected_object")" == "600 1" ]] ||
      die "selected scorecard object mode is invalid"
    [[ "$(sha256sum -- "$selected_object" | cut -d ' ' -f 1)" == "$sha256" ]] ||
      die "selected scorecard object content is invalid"
  }

  # require every v2 hot object to remain selected or part of this exact retry
  validate_v2_hot_objects() {
    local candidate
    local candidate_name
    local candidate_sha256
    local retained
    local selected_sha256
    local -a candidates
    shopt -s nullglob dotglob
    candidates=("$scorecard_root/scorecards-v2"/*)
    shopt -u nullglob dotglob

    # reject malformed, linked or unanchored hot v2 objects
    for candidate in "${candidates[@]}"; do
      candidate_name=${candidate##*/}
      [[ "$candidate_name" =~ ^sha256-([a-f0-9]{64})\.json$ ]] ||
        die "v2 scorecard object directory contains an invalid entry"
      candidate_sha256=${BASH_REMATCH[1]}
      [[ -f "$candidate" && ! -L "$candidate" ]] ||
        die "v2 scorecard object is invalid"
      [[ "$(stat -c '%a %h' -- "$candidate")" == "600 1" ]] ||
        die "v2 scorecard object mode or link count is invalid"
      [[ "$(sha256sum -- "$candidate" | cut -d ' ' -f 1)" == "$candidate_sha256" ]] ||
        die "v2 scorecard object content is invalid"
      retained=false

      # permit only the three fixed slots or this exact recoverable publication
      for selected_sha256 in "$current_sha256" "$previous_sha256" "$pending_sha256" "$expected_sha256"; do
        if [[ -n "$selected_sha256" && "$candidate_sha256" == "$selected_sha256" ]]; then
          retained=true
        fi
      done
      [[ "$retained" == true ]] ||
        die "unanchored v2 scorecard object requires archive retirement recovery"
    done
  }

  # publish one hash-only pointer through its private temporary file
  write_pointer() {
    local output=$1
    local sha256=$2
    printf '{"sha256":"%s"}\n' "$sha256" >"$output"
    chown "$runtime_uid:$runtime_gid" "$output"
    chmod 0600 "$output"
    sync -f "$output"

    # prove the runtime identity can read the pointer before replacement
    setpriv --reuid="$runtime_uid" --regid="$runtime_gid" --clear-groups \
      test -r "$output"
  }

  # read one existing fixed private hash pointer without following links
  read_pointer() {
    "$native_evidence_binary" read-scorecard-install-pointer "${1##*/}"
  }

  # retain the historical v1 pointer behavior unchanged
  if [[ "$install_mode" == v1 ]]; then
    publish_object
    write_pointer "$pointer_temporary" "$expected_sha256"
    mv -Tf -- "$pointer_temporary" "$scorecard_root/current.json"
    sync -f "$scorecard_root"

    # prove the selected pointer remains readable after atomic replacement
    setpriv --reuid="$runtime_uid" --regid="$runtime_gid" --clear-groups \
      test -r "$scorecard_root/current.json"
    trap - EXIT
    rm -f -- "$previous_temporary"
    printf 'Installed forecast adjustment scorecard: %s\n' "$expected_sha256"
    exit 0
  fi

  v2_current="$scorecard_root/v2-current.json"
  v2_previous="$scorecard_root/v2-previous.json"
  v2_pending="$scorecard_root/v2-pending.json"
  current_sha256=
  previous_sha256=
  pending_sha256=
  legacy_sha256=

  # freeze and validate every existing v2 selection before mutation
  if [[ -e "$v2_current" || -L "$v2_current" ]]; then
    current_sha256=$(read_pointer "$v2_current")
  fi
  if [[ -e "$v2_previous" || -L "$v2_previous" ]]; then
    previous_sha256=$(read_pointer "$v2_previous")
  fi
  if [[ -e "$v2_pending" || -L "$v2_pending" ]]; then
    pending_sha256=$(read_pointer "$v2_pending")
    [[ "$pending_sha256" == "$expected_sha256" ]] ||
      die "another v2 scorecard installation is pending"
  fi
  if [[ -e "$scorecard_root/current.json" || -L "$scorecard_root/current.json" ]]; then
    legacy_sha256=$(read_pointer "$scorecard_root/current.json")
  fi

  # bind every fixed pointer to an existing immutable object before mutation
  for selected_sha256 in "$current_sha256" "$previous_sha256" "$pending_sha256"; do
    if [[ -n "$selected_sha256" ]]; then
      verify_selected_object "$scorecard_root/scorecards-v2" "$selected_sha256"
    fi
  done
  if [[ -n "$legacy_sha256" ]]; then
    verify_selected_object "$scorecard_root/scorecards" "$legacy_sha256"
  fi
  validate_v2_hot_objects

  # retain legacy v1 and only one pending v2 projection during migration
  if [[ -n "$legacy_sha256" ]]; then
    [[ -z "$current_sha256" && -z "$previous_sha256" ]] ||
      die "legacy v1 cannot coexist with selected v2 scorecards"
    publish_object

    # make the exact pending migration publication idempotent
    if [[ -n "$pending_sha256" ]]; then
      trap - EXIT
      rm -f -- "$pointer_temporary" "$previous_temporary"
      printf 'Installed pending forecast adjustment scorecard v2: %s\n' "$expected_sha256"
      exit 0
    fi

    write_pointer "$pointer_temporary" "$expected_sha256"
    mv -Tf -- "$pointer_temporary" "$v2_pending"
    sync -f "$scorecard_root"
    trap - EXIT
    rm -f -- "$previous_temporary"
    printf 'Installed pending forecast adjustment scorecard v2: %s\n' "$expected_sha256"
    exit 0
  fi

  # refuse a third hot v2 object without an exact anchored cold retirement
  if [[ -z "$pending_sha256" && -n "$current_sha256" && -n "$previous_sha256" &&
    "$expected_sha256" != "$current_sha256" && "$expected_sha256" != "$previous_sha256" ]]; then
    die "v2 scorecard retirement authority is required before another installation"
  fi

  # refuse recovery that would overwrite an unarchived previous selection
  if [[ -n "$pending_sha256" && -n "$previous_sha256" &&
    "$previous_sha256" != "$current_sha256" && "$current_sha256" != "$expected_sha256" ]]; then
    die "pending v2 rotation would discard an unarchived previous scorecard"
  fi

  publish_object

  # make an already selected publication idempotent
  if [[ "$current_sha256" == "$expected_sha256" && -z "$pending_sha256" ]]; then
    trap - EXIT
    rm -f -- "$pointer_temporary" "$previous_temporary"
    printf 'Installed forecast adjustment scorecard v2: %s\n' "$expected_sha256"
    exit 0
  fi

  # commit the recoverable pending selection before rotating current and previous
  if [[ -z "$pending_sha256" ]]; then
    write_pointer "$pointer_temporary" "$expected_sha256"
    mv -Tf -- "$pointer_temporary" "$v2_pending"
    sync -f "$scorecard_root"
    pending_sha256=$expected_sha256
  fi

  # preserve the former current selection unless a prior interrupted rotation already did
  if [[ -n "$current_sha256" && "$current_sha256" != "$expected_sha256" &&
    "$previous_sha256" != "$current_sha256" ]]; then
    write_pointer "$previous_temporary" "$current_sha256"
    mv -Tf -- "$previous_temporary" "$v2_previous"
    sync -f "$scorecard_root"
  fi

  # select only the exact pending object through one atomic rename
  mv -Tf -- "$v2_pending" "$v2_current"
  sync -f "$scorecard_root"

  # prove the selected pointer remains readable after atomic replacement
  [[ "$(read_pointer "$v2_current")" == "$expected_sha256" ]] ||
    die "v2 scorecard selection did not persist"
  setpriv --reuid="$runtime_uid" --regid="$runtime_gid" --clear-groups \
    test -r "$v2_current"
  trap - EXIT
  rm -f -- "$pointer_temporary" "$previous_temporary"
  printf 'Installed forecast adjustment scorecard v2: %s\n' "$expected_sha256"
)

# execute a verified family image switch under the same release transaction lock
native_gateway_family_apply() (
  (($# == 8)) || native_gateway_fail "native family switch operands differ"
  local release=$1 expected=$2 alternate=$3 commit=$4 catalog=$5 family_identity=$6 images=$7 action=$8
  local descriptor family role current target paired publication snapshot
  # require the exact inherited native source before reading the mutation journal
  native_gateway_context --inherited-family-lock
  [[ "$native_active_generation" == rust-v1 ]] || native_gateway_fail "native family source generation differs"
  native_gateway_package "$native_active_commit"
  validate_release "$release"
  validate_release "$expected"
  validate_release "$alternate"
  [[ "$commit" =~ ^[a-f0-9]{40}$ && "$catalog" =~ ^[a-f0-9]{64}$ &&
    "$family_identity" =~ ^[a-f0-9]{64}$ && "$images" =~ ^[a-f0-9]{64}$ &&
    "$action" =~ ^[a-f0-9]{64}$ ]] || native_gateway_fail "native family switch identity differs"
  descriptor=$("$native_evidence_binary" verify-family-switch-record "$@")
  IFS=$'\t' read -r family role <<<"$descriptor"
  [[ "$family" =~ ^(temperature|wind|rain)$ && "$role" =~ ^(target|compensation)$ ]] ||
    native_gateway_fail "native family switch receipt differs"
  current=$(env_value "$native_active_environment" WEATHER_RELEASE)
  [[ "$current" == "$expected" || "$current" == "$alternate" || "$current" == "$release" ]] ||
    native_gateway_fail "native family current source differs"
  target=$(sp_target "$release")
  [[ "$(sp_runtime_generation "$target")" == rust-v1 ]] || native_gateway_fail "native family target generation differs"
  publication="$sp_root/$release/publication.json"
  sp_private "$publication"
  # admit only the independently staged reciprocal native pair for the retained mutation
  [[ "$(sp_field "$publication" contractVersion)" == weather-schema-preserving-family-publication/v1 &&
    "$(sp_field "$publication" sourceRelease)" == "$expected" &&
    "$(sp_field "$publication" targetCommit)" == "$commit" &&
    "$(sp_field "$publication" family)" == "$family" &&
    "$(sp_field "$publication" familyRole)" == "$role" &&
    "$(sp_field "$publication" familyActionSha256)" == "$action" ]] ||
    native_gateway_fail "native family target stage differs"
  paired=$(sp_field "$publication" pairedRelease)
  sp_native_family_pair "$release" "$paired" || native_gateway_fail "native family staged pair differs"
  sp_native_host_readiness "$commit"
  [[ "$(sp_sha "$state_dir/adjustment-candidate-catalog.json")" == "$catalog" ]] ||
    native_gateway_fail "native family catalog differs"
  # a failed family image transaction may be superseded only by its exact durable compensation
  if [[ -e "$sp_journal" || -L "$sp_journal" ]]; then
    [[ "$role" == compensation ]] || native_gateway_fail "interrupted family target requires compensation"
    sp_native_family_resume_compensation "$release" "$paired" "$action" "$catalog"
  fi
  # prove the already committed exact release on retries without another image replacement
  if [[ "$current" == "$release" ]]; then
    sp_database "$target"
    sp_health "$target"
    return
  fi
  # retain the freshly installed catalog and all other protected state across the native switch
  snapshot="$sp_root/$release/rollback-protected-${EPOCHSECONDS}-${BASHPID}.json"
  sp_snapshot "$native_active_environment" "$snapshot"
  sp_transaction "$release" "$release" "$snapshot"
)

# measure the original six immutable roles before the first paired native image pull
native_gateway_family_capacity() (
  (($# == 9)) || native_gateway_fail "family capacity operands differ"
  native_gateway_context --inherited-family-lock
  [[ "$native_active_generation" == rust-v1 &&
    "$3" == "$(env_value "$native_active_environment" WEATHER_RELEASE)" &&
    "$4" == "$(env_value "$native_active_environment" WEATHER_SERVER_IMAGE)" &&
    "$5" == "$(env_value "$native_active_environment" WEATHER_WEB_IMAGE)" ]] ||
    native_gateway_fail "family capacity source differs"
  native_gateway_package "$native_active_commit"
  "$native_evidence_binary" family-release-capacity "$@"
)

# reconcile a crash-left family record under the existing boot recovery lock
native_gateway_family_recover() (
  (($# == 0)) || native_gateway_fail "family recovery takes no operands"
  native_gateway_context --inherited-family-lock
  [[ "$native_active_generation" == rust-v1 ]] || native_gateway_fail "family recovery generation differs"
  native_gateway_package "$native_active_commit"
  "$native_evidence_binary" recover-family-release
)

# invoke the native family transaction with the original nine operands and inherited lock
native_host_family_release() {
  (($# == 9)) || native_gateway_fail "native family release operands differ"
  # the authenticated auxiliary controller holds the release lock descriptor
  # shellcheck disable=SC2154
  export WEATHER_RELEASE_LOCK_FD=$release_transaction_lock_fd
  "$native_evidence_binary" family-release "$@"
}

# recover the exact auxiliary release journal even if its activation link was interrupted
native_gateway_system_recover() (
  (($# == 0)) || native_gateway_fail "recover takes no operands"
  native_gateway_context --system-recovery
  sp_system_recover
)

# recover the exact dashboard web state with the original core lifecycle lock held
native_gateway_dashboard_recover() (
  (($# == 0)) || native_gateway_fail "dashboard recovery takes no operands"
  native_gateway_context --inherited-dashboard-lock
  [[ "$native_active_generation" == rust-v1 ]] || native_gateway_fail "dashboard recovery generation differs"
  native_gateway_package "$native_active_commit"
  # shellcheck disable=SC1090,SC1091
  source "$native_evidence_package/native-dashboard-release-control.sh"
  native_dashboard_system_recover "$@"
)

# restrict override-aware loading to the three existing dashboard mutations
native_gateway_dashboard_action() {
  case "${1-}" in
    dashboard-release|dashboard-rollback|dashboard-rollback-core) return 0 ;;
    *) return 1 ;;
  esac
}

# keep core-changing verbs behind the original dashboard rollback boundary
native_gateway_core_action() {
  case "${1-}" in
    stage|activate|yolo|rollback|adjustment-family-release) return 0 ;;
    *) return 1 ;;
  esac
}

# retain the original installed Node gateway byte-for-byte for rollback
native_gateway_legacy() {
  native_gateway_file "$native_legacy_dispatch" 755
  native_gateway_file "$native_evidence_root/legacy-remote-ops.sha256" 600
  [[ "$(sha256sum "$native_legacy_dispatch" | awk '{print $1}')" == "$(cat "$native_evidence_root/legacy-remote-ops.sha256")" ]] ||
    native_gateway_fail "retained predecessor gateway differs"
  exec "$native_legacy_dispatch" "$@"
}

# route only the existing command vocabulary through the selected runtime generation
native_gateway_main() {
  [[ "$EUID" == 0 && $# -ge 1 ]] || native_gateway_fail "remote operations require their installed owner"
  local generation commit descriptor diagnostic status
  # recovery belongs to the effective auxiliary controller on either side of a native switch
  if [[ "$1" == recover ]]; then
    shift
    native_gateway_system_recover "$@"
    return
  fi
  # this internal continuation is never admitted by the public forced-command grammar
  if [[ "$1" == --family-apply-native-unlocked ]]; then
    shift
    native_gateway_family_apply "$@"
    return
  elif [[ "$1" == --family-recover-native-unlocked ]]; then
    shift
    native_gateway_family_recover "$@"
    return
  elif [[ "$1" == --family-capacity-native-unlocked ]]; then
    shift
    native_gateway_family_capacity "$@"
    return
  elif [[ "$1" == --dashboard-recover-native-unlocked ]]; then
    shift
    native_gateway_dashboard_recover "$@"
    return
  fi
  # this internal root check is unavailable through the retained forced-command grammar
  if [[ "$1" == --verify-native-package && $# == 2 ]]; then
    native_gateway_file "$native_gateway" 755
    native_gateway_package "$2"
    [[ "$(sha256sum "$native_gateway" | awk '{print $1}')" == \
      "$(sha256sum "$native_evidence_package/native-generation-remote-ops.sh" | awk '{print $1}')" ]] ||
      native_gateway_fail "installed native gateway differs"
    # execute the closed no-operand diagnostic to prove the actual host ABI without touching application state
    status=0
    diagnostic=$("$native_evidence_binary" 2>&1) || status=$?
    [[ "$status" == 1 && "$diagnostic" == "weather-evidence: missing evidence command" ]] ||
      native_gateway_fail "installed native evidence executable readiness differs"
    return
  fi
  # the original family operation can resume only its previously interrupted native transaction
  if [[ "$1" == adjustment-family-release ]]; then
    descriptor=$(native_gateway_generation --family-recovery) || native_gateway_fail "active family generation unavailable"
  elif native_gateway_dashboard_action "$1"; then
    descriptor=$(native_gateway_generation --dashboard-override) || native_gateway_fail "active dashboard generation unavailable"
  elif native_gateway_core_action "$1"; then
    descriptor=$(native_gateway_generation) || native_gateway_fail "active application generation unavailable"
  else
    descriptor=$(native_gateway_generation --committed-dashboard-context) || native_gateway_fail "active application generation unavailable"
  fi
  IFS=$'\t' read -r generation commit <<<"$descriptor"
  if [[ "$generation" == node-v1 ]]; then
    native_gateway_legacy "$@"
  fi
  [[ "$generation" == rust-v1 ]] || native_gateway_fail "native generation required"
  native_gateway_package "$commit"
  # retain exact native operation semantics and the existing root/service account split
  # shellcheck disable=SC1090,SC1091
  source "$native_evidence_package/native-evidence-remote-dispatch.sh"
  # shellcheck disable=SC1090,SC1091
  source "$native_evidence_package/native-release-control-remote-dispatch.sh"
  # shellcheck disable=SC1090,SC1091
  source "$native_evidence_package/native-dashboard-release-control.sh"
  if [[ "$1" == public-stations-backfill || "$1" == tide-backfill || "$1" == tempest-backfill ]] || native_evidence_handles "$1" || native_release_control_handles "$1"; then
    native_gateway_execute "$commit" "$@"
    exit $?
  fi
  native_gateway_legacy "$@"
}

# allow focused local fixture sourcing without touching installed host paths
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  native_gateway_main "$@"
fi
