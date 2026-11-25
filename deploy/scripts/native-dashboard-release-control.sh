#!/usr/bin/env bash

# shellcheck disable=SC2154

readonly native_dashboard_publication_root=/opt/weather/operators/native-dashboard-publications

# report one closed native dashboard owner failure
native_dashboard_fail() {
  printf 'error: native dashboard release differs: %s\n' "$*" >&2
  return 1
}

# read one admitted scalar from the authenticated dashboard validator
native_dashboard_field() {
  node "$sp_module" field "$1" "$2"
}

# resolve one fixed root-owned web-only publication
native_dashboard_publication() {
  local target=$1 core=$2 source=$3 base directory publication checksum digest line bytes path
  base="$native_dashboard_publication_root/by-release"
  directory="$base/$target"
  publication="$directory/publication.json"
  checksum="$directory/publication.sha256"
  # bind every directory to the fixed root-owned namespace
  for path in "$native_dashboard_publication_root" "$base" "$directory"; do
    [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" &&
      "$(stat -c '%u:%g:%a' -- "$path")" == 0:0:700 ]] || {
      native_dashboard_fail "publication directory"
      return
    }
  done
  sp_private "$publication" || return
  sp_private "$checksum" || return
  digest=$(sp_sha "$publication")
  line=$(cat -- "$checksum")
  bytes=$(wc -c <"$checksum")
  # require one exact checksum line and all explicit release coordinates
  [[ "$bytes" == 83 && "$line" == "$digest  publication.json" ]] || {
    native_dashboard_fail "publication checksum"
    return
  }
  node "$sp_module" validate-dashboard-publication \
    "$publication" "$target" "$core" "$source" >/dev/null || {
    native_dashboard_fail "publication contract"
    return
  }
  printf '%s\t%s\n' "$publication" "$digest"
}

# select one committed core or retained dashboard source environment
native_dashboard_source_environment() {
  local core=$1 source=$2 core_environment current_web environment
  core_environment=$(sp_source) || return
  [[ "$(env_value "$core_environment" WEATHER_RELEASE)" == "$core" &&
    "$(sp_runtime_generation "$core_environment")" == rust-v1 ]] || {
    native_dashboard_fail "active core"
    return
  }
  current_web=$(read_optional_release_state "$state_dir/current-web-release")
  # select the unchanged core web only when no override marker exists
  if [[ "$source" == "$core" ]]; then
    [[ -z "$current_web" && ! -e "$state_dir/active-web.env" &&
      ! -L "$state_dir/active-web.env" ]] || {
      native_dashboard_fail "core web source markers"
      return
    }
    printf '%s\n' "$core_environment"
    return
  fi
  [[ "$current_web" == "$source" ]] || {
    native_dashboard_fail "override source marker"
    return
  }
  environment=$(dashboard_release_env "$source")
  validate_dashboard_release_env "$environment" "$source" "$core" || return
  [[ -L "$state_dir/active-web.env" &&
    "$(readlink "$state_dir/active-web.env")" == "../releases/$source.web.env" ]] || {
    native_dashboard_fail "override source link"
    return
  }
  printf '%s\n' "$environment"
}

# resolve the actual publication and commit behind one current web source
native_dashboard_source_publication() {
  local core=$1 source=$2 publication source_of_source
  # the core web inherits the exact ordinary core publication
  if [[ "$source" == "$core" ]]; then
    publication="$sp_root/$core/publication.json"
    sp_private "$publication" || return
    printf '%s\t%s\t%s\n' "$publication" "$(sp_sha "$publication")" \
      "$(sp_field "$publication" targetCommit)"
    return
  fi
  publication="$native_dashboard_publication_root/by-release/$source/publication.json"
  source_of_source=$(native_dashboard_field "$publication" sourceRelease) || return
  native_dashboard_publication "$source" "$core" "$source_of_source" >/dev/null || return
  printf '%s\t%s\t%s\n' "$publication" "$(sp_sha "$publication")" \
    "$(native_dashboard_field "$publication" targetCommit)"
}

# bind the retained web image reference and local image ID to its release proof
native_dashboard_image_identity() {
  local core=$1 release=$2 coordinates directory stage_core stage_source protected publication image image_id
  if [[ "$release" == "$core" ]]; then
    publication="$sp_root/$core/publication.json"
    sp_private "$publication" || return
    image=$(sp_field "$publication" webImage) || return
    image_id=$(sp_field "$publication" webImageId) || return
  else
    coordinates=$(native_dashboard_stage_receipt "$release") || return
    IFS=$'\t' read -r directory stage_core stage_source protected <<<"$coordinates"
    [[ "$stage_core" == "$core" ]] || { native_dashboard_fail "web stage core"; return; }
    publication="$directory/publication.json"
    image=$(native_dashboard_field "$publication" targetWebImage) || return
    image_id=$(native_dashboard_field "$publication" targetWebImageId) || return
  fi
  sp_image "$image" "$image_id" || return
  printf '%s\t%s\n' "$image" "$image_id"
}

# prove the selected environment uses the exact retained release image
native_dashboard_environment_image_id() {
  local core=$1 release=$2 environment=$3 identity image image_id
  identity=$(native_dashboard_image_identity "$core" "$release") || return
  IFS=$'\t' read -r image image_id <<<"$identity"
  [[ "$image" == "$(env_value "$environment" WEATHER_WEB_IMAGE)" ]] || {
    native_dashboard_fail "web environment image proof"
    return
  }
  printf '%s\n' "$image_id"
}

# bind one target publication to the independently observed core and incumbent web
native_dashboard_live_admission() {
  local publication=$1 core=$2 source=$3 core_environment source_environment
  local core_publication source_coordinates source_publication source_sha source_commit
  local source_image source_image_id expected
  core_environment=$(sp_source) || return
  source_environment=$(native_dashboard_source_environment "$core" "$source") || return
  core_publication="$sp_root/$core/publication.json"
  sp_private "$core_publication" || return
  source_coordinates=$(native_dashboard_source_publication "$core" "$source") || return
  IFS=$'\t' read -r source_publication source_sha source_commit <<<"$source_coordinates"
  source_image=$(env_value "$source_environment" WEATHER_WEB_IMAGE)
  source_image_id=$(native_dashboard_environment_image_id "$core" "$source" "$source_environment") || return
  expected="$(native_dashboard_field "$publication" coreRelease)|$(native_dashboard_field "$publication" corePublicationSha256)|$(native_dashboard_field "$publication" coreCommit)|$(native_dashboard_field "$publication" sourceRelease)|$(native_dashboard_field "$publication" sourcePublicationSha256)|$(native_dashboard_field "$publication" sourceCommit)|$(native_dashboard_field "$publication" sourceWebImage)|$(native_dashboard_field "$publication" sourceWebImageId)"
  # compare every core and source coordinate to independent live owners
  [[ "$expected" == "$core|$(sp_sha "$core_publication")|$(sp_field "$core_publication" targetCommit)|$source|$source_sha|$source_commit|$source_image|$source_image_id" ]] || {
    native_dashboard_fail "live core or source binding"
    return
  }
  sp_database "$core_environment" || return
  native_dashboard_health "$core_environment" "$source_environment" "$source" "$source_image" "$source_image_id"
}

# render the native compose override against a web-only environment
native_dashboard_compose() (
  local core_environment=$1 web_environment=$2 override
  shift 2
  [[ "$(sp_runtime_generation "$core_environment")" == rust-v1 ]] || {
    native_dashboard_fail "core generation"
    return
  }
  override=$(mktemp "$state_dir/.native-dashboard-compose.XXXXXX.yaml")
  trap 'rm -f -- "$override"' EXIT
  node "$sp_module" runtime-compose-override rust-v1 >"$override"
  chmod 600 "$override"
  WEATHER_ENV_FILE="$web_environment" compose -f "$compose_file" -f "$override" "$@"
)

# verify one running native web image and its ordinary readable boundary
native_dashboard_health() {
  local core_environment=$1 web_environment=$2 release=$3 image=$4 image_id=$5 container actual
  sp_image "$image" "$image_id" || return
  container=$(native_dashboard_compose "$core_environment" "$web_environment" ps -q web) || return
  [[ "$container" =~ ^[a-f0-9]{64}$ ]] || {
    native_dashboard_fail "web container identity"
    return
  }
  actual=$(docker inspect --format \
    '{{.Config.Image}}|{{.Image}}|{{.State.Running}}|{{.State.Health.Status}}' "$container") || return
  [[ "$actual" == "$image|$image_id|true|healthy" ]] || {
    native_dashboard_fail "web container runtime"
    return
  }
  native_dashboard_compose "$core_environment" "$web_environment" exec -T web \
    /opt/weather/bin/weather-web-health web-release "$release"
}

# capture the stable core, database and infrastructure boundary around one web-only change
native_dashboard_snapshot() (
  local core_environment=$1 output=$2 base current previous schema active service container runtime
  base=$(mktemp "$sp_root/native-dashboard/.core-snapshot.XXXXXX") || return
  rm -- "$base" || return
  trap 'rm -f -- "$base"' EXIT
  sp_snapshot "$core_environment" "$base" || return
  current=$(read_release_state "$state_dir/current-release") || return
  previous=$(read_optional_release_state "$state_dir/previous-release") || return
  schema=$(read_release_state "$state_dir/schema-release") || return
  active=$(readlink "$state_dir/active.env") || return
  validate_release "$current" || return
  [[ -z "$previous" ]] || validate_release "$previous" || return
  validate_release "$schema" || return
  [[ "$active" == "../releases/$current.env" &&
    "$core_environment" == "$(release_env "$current")" ]] || {
    native_dashboard_fail "core marker snapshot"
    return
  }
  umask 077
  printf '%s\n' \
    'NATIVE_DASHBOARD_PROTECTED_VERSION=weather-native-dashboard-protected/v1' \
    "NATIVE_DASHBOARD_BASE_SHA256=$(sp_sha "$base")" \
    "NATIVE_DASHBOARD_CURRENT_RELEASE=$current" \
    "NATIVE_DASHBOARD_PREVIOUS_RELEASE=${previous:-none}" \
    "NATIVE_DASHBOARD_SCHEMA_RELEASE=$schema" \
    "NATIVE_DASHBOARD_ACTIVE_ENV=$active" \
    "NATIVE_DASHBOARD_ACTIVE_ENV_SHA256=$(sp_sha "$core_environment")" >"$output" || return
  # retain the exact non-web application container configuration without volatile clocks
  for service in api worker; do
    container=$(WEATHER_ENV_FILE="$core_environment" compose ps -a -q "$service") || return
    [[ "$container" =~ ^[a-f0-9]{64}$ ]] || {
      native_dashboard_fail "core $service container identity"
      return
    }
    runtime=$(docker inspect --format \
      '{{.Id}}|{{.Image}}|{{.Config.Image}}|{{.HostConfig.Memory}}|{{.HostConfig.MemorySwap}}|{{.HostConfig.MemoryReservation}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.CpuPeriod}}|{{.HostConfig.CpuQuota}}' \
      "$container") || return
    [[ "$runtime" == "$container|"* ]] || {
      native_dashboard_fail "core $service runtime identity"
      return
    }
    printf 'NATIVE_DASHBOARD_%s_RUNTIME=%s\n' "${service^^}" "$runtime" >>"$output" || return
  done
  chmod 600 "$output" || return
  rm -- "$base" || return
  trap - EXIT
)

# compare the complete stable dashboard boundary after one web-only operation
native_dashboard_unchanged() {
  local core_environment=$1 expected=$2 actual
  actual=$(mktemp "$sp_root/native-dashboard/.core-compare.XXXXXX")
  rm -- "$actual"
  native_dashboard_snapshot "$core_environment" "$actual" || return
  cmp -s "$expected" "$actual" || {
    rm -f -- "$actual"
    native_dashboard_fail "protected core changed"
    return
  }
  rm -- "$actual"
}

# verify one committed web against a fresh core boundary and optionally restart only web
native_dashboard_verify_committed_web() (
  local core_environment=$1 web_environment=$2 release=$3 image=$4 image_id=$5 restart=$6
  local snapshot
  snapshot=$(mktemp "$sp_root/native-dashboard/.committed-protected.XXXXXX")
  rm -- "$snapshot"
  trap 'rm -f -- "$snapshot"' EXIT
  native_dashboard_snapshot "$core_environment" "$snapshot" || return
  # boot recovery restarts only web while an ordinary retry observes the already-running web
  if [[ "$restart" == true ]]; then
    native_dashboard_compose "$core_environment" "$web_environment" \
      up -d --no-deps --no-build --pull never --wait web || return
  elif [[ "$restart" != false ]]; then
    native_dashboard_fail "committed web restart mode"
    return
  fi
  native_dashboard_health "$core_environment" "$web_environment" \
    "$release" "$image" "$image_id" || return
  native_dashboard_unchanged "$core_environment" "$snapshot"
)

# admit one retained stage receipt and its immutable publication
native_dashboard_stage_receipt() {
  local target=$1 directory receipt publication protected core source proof_sha target_env_sha
  local before after server source_web target_web
  directory="$sp_root/native-dashboard/$target"
  receipt="$directory/stage.env"
  publication="$directory/publication.json"
  protected="$directory/protected.json"
  before="$directory/capacity-before.json"
  after="$directory/capacity-after.json"
  sp_private "$receipt" || return
  sp_private "$publication" || return
  sp_private "$protected" || return
  sp_private "$before" || return
  sp_private "$after" || return
  # reject missing, duplicated and unknown stage fields
  if [[ "$(grep -cE '^[A-Z][A-Z0-9_]*=' "$receipt")" != 8 ]] ||
    grep -qEv '^(NATIVE_DASHBOARD_(CONTRACT|CORE_RELEASE|SOURCE_RELEASE|PUBLICATION_SHA256|TARGET_ENV_SHA256|PROTECTED_SHA256|CAPACITY_BEFORE_SHA256|CAPACITY_AFTER_SHA256))=' "$receipt"; then
    native_dashboard_fail "stage receipt shape"
    return
  fi
  core=$(env_value "$receipt" NATIVE_DASHBOARD_CORE_RELEASE)
  source=$(env_value "$receipt" NATIVE_DASHBOARD_SOURCE_RELEASE)
  proof_sha=$(env_value "$receipt" NATIVE_DASHBOARD_PUBLICATION_SHA256)
  target_env_sha=$(env_value "$receipt" NATIVE_DASHBOARD_TARGET_ENV_SHA256)
  [[ "$(env_value "$receipt" NATIVE_DASHBOARD_CONTRACT)" == weather-native-dashboard-stage/v1 &&
    "$proof_sha" == "$(sp_sha "$publication")" &&
    "$target_env_sha" == "$(sp_sha "$(dashboard_release_env "$target")")" &&
    "$(env_value "$receipt" NATIVE_DASHBOARD_PROTECTED_SHA256)" == "$(sp_sha "$protected")" &&
    "$(env_value "$receipt" NATIVE_DASHBOARD_CAPACITY_BEFORE_SHA256)" == "$(sp_sha "$before")" &&
    "$(env_value "$receipt" NATIVE_DASHBOARD_CAPACITY_AFTER_SHA256)" == "$(sp_sha "$after")" ]] || {
    native_dashboard_fail "stage receipt identity"
    return
  }
  native_dashboard_publication "$target" "$core" "$source" >/dev/null || return
  [[ "$(sp_sha "$native_dashboard_publication_root/by-release/$target/publication.json")" == "$proof_sha" ]] || {
    native_dashboard_fail "stage publication changed"
    return
  }
  # retain both actual measurement decisions with the exact stage publication and images
  server=$(env_value "$(release_env "$core")" WEATHER_SERVER_IMAGE) || return
  source_web=$(native_dashboard_field "$publication" sourceWebImage) || return
  target_web=$(native_dashboard_field "$publication" targetWebImage) || return
  node "$sp_module" validate-dashboard-capacity "$before" "$proof_sha" "$core" "$server" "$source_web" "$target_web" >/dev/null || return
  node "$sp_module" validate-dashboard-capacity "$after" "$proof_sha" "$core" "$server" "$source_web" "$target_web" >/dev/null || return
  printf '%s\t%s\t%s\t%s\n' "$directory" "$core" "$source" "$protected"
}

# measure live Docker allocation with the installed native six-role owner
native_dashboard_measure_capacity() (
  local output=$1 proof=$2 core=$3 server=$4 source_web=$5 target_web=$6 status=0
  umask 077
  "$native_evidence_binary" dashboard-release-capacity "$proof" "$core" "$server" "$source_web" "$target_web" >"$output" || status=$?
  # retain measured denial diagnostics without granting a pull
  if ((status != 0)); then
    cat -- "$output" >&2
    return "$status"
  fi
  chmod 600 "$output" || return
  node "$sp_module" validate-dashboard-capacity "$output" "$proof" "$core" "$server" "$source_web" "$target_web" >/dev/null
)

# remeasure a retained stage when its physical capacity observation is no longer current
native_dashboard_activation_capacity() (
  local directory=$1 proof=$2 core=$3 publication=$4 environment=$5 measured measured_epoch now
  local refresh digest retained server source_web target_web
  sp_capacity || return
  measured=$(native_dashboard_field "$directory/capacity-after.json" measuredAt) || return
  measured_epoch=$(date -u -d "$measured" +%s) || return
  now=$(date -u +%s) || return
  # the immediately post-pull measurement remains sufficient for a fresh activation
  if ((now >= measured_epoch && now - measured_epoch <= 900)); then return; fi
  server=$(env_value "$environment" WEATHER_SERVER_IMAGE) || return
  source_web=$(native_dashboard_field "$publication" sourceWebImage) || return
  target_web=$(native_dashboard_field "$publication" targetWebImage) || return
  refresh=$(mktemp "$directory/.capacity-refresh.XXXXXX") || return
  trap 'rm -f -- "$refresh"' EXIT
  native_dashboard_measure_capacity "$refresh" "$proof" "$core" "$server" "$source_web" "$target_web" || return
  digest=$(sp_sha "$refresh") || return
  retained="$directory/capacity-refresh-$digest.json"
  # retain the actual fresh capacity decision independently of the immutable stage metadata
  if [[ -e "$retained" || -L "$retained" ]]; then
    sp_private "$retained" || return
    [[ "$(sp_sha "$retained")" == "$digest" ]] || return 1
  else
    mv -- "$refresh" "$retained" || return
    sync -f "$retained" "$directory" || return
  fi
)

# stage one web image and private environment without changing live markers
native_dashboard_stage() (
  local target=$1 core=$2 source=$3 coordinates publication proof_sha directory
  local core_environment target_environment target_image target_image_id target_commit markers
  local protected temporary staging stage_root server source_web
  coordinates=$(native_dashboard_publication "$target" "$core" "$source") || return
  IFS=$'\t' read -r publication proof_sha <<<"$coordinates"
  stage_root="$sp_root/native-dashboard"
  directory="$stage_root/$target"
  # admit only an identical already completed stage
  if [[ -e "$directory/stage.env" || -L "$directory/stage.env" ]]; then
    native_dashboard_stage_receipt "$target" >/dev/null || return
    # the committed active retry is verified by activation rather than its historical source
    if [[ "$(read_optional_release_state "$state_dir/current-web-release")" != "$target" ]]; then
      native_dashboard_live_admission "$publication" "$core" "$source" || return
    fi
    printf 'Dashboard web release %s is already staged.\n' "$target"
    return
  fi
  native_dashboard_live_admission "$publication" "$core" "$source" || return
  [[ ! -e "$directory" && ! -L "$directory" ]] || {
    native_dashboard_fail "partial stage"
    return
  }
  # build the receipt privately so a failed pull never leaves a committed partial stage
  # create only an absent private stage directory and admit existing exact metadata
  if [[ ! -e "$stage_root" && ! -L "$stage_root" ]]; then
    install -d -m 0700 -o 0 -g 0 "$stage_root" || return
  fi
  [[ -d "$stage_root" && ! -L "$stage_root" && "$(realpath -e -- "$stage_root")" == "$stage_root" &&
    "$(stat -c '%u:%g:%a' -- "$stage_root")" == 0:0:700 ]] || { native_dashboard_fail "stage root"; return; }
  staging=$(mktemp -d "$stage_root/.stage-$target.XXXXXX") || return
  # remove only the unpublished files owned by this attempt
  trap 'if [[ -d "$staging" ]]; then rm -f -- "$staging/publication.json" "$staging/protected.json" "$staging/capacity-before.json" "$staging/capacity-after.json" "$staging/.stage.env.partial" "$staging/stage.env"; rmdir -- "$staging"; fi' EXIT
  cp -- "$publication" "$staging/publication.json" || return
  chmod 600 "$staging/publication.json" || return
  core_environment=$(sp_source) || return
  protected="$staging/protected.json"
  native_dashboard_snapshot "$core_environment" "$protected" || return
  sp_capacity || return
  target_environment=$(dashboard_release_env "$target") || return
  target_image=$(native_dashboard_field "$publication" targetWebImage) || return
  target_image_id=$(native_dashboard_field "$publication" targetWebImageId) || return
  target_commit=$(native_dashboard_field "$publication" targetCommit) || return
  server=$(env_value "$core_environment" WEATHER_SERVER_IMAGE) || return
  source_web=$(native_dashboard_field "$publication" sourceWebImage) || return
  # create or compare only the exact web-only environment
  if [[ -e "$target_environment" || -L "$target_environment" ]]; then
    validate_dashboard_release_env "$target_environment" "$target" "$core" || return
    [[ "$(env_value "$target_environment" WEATHER_WEB_IMAGE)" == "$target_image" ]] || {
      native_dashboard_fail "target environment image"
      return
    }
  else
    write_dashboard_release_env "$core_environment" "$target_environment" "$target" "$target_image" || return
  fi
  native_dashboard_measure_capacity "$staging/capacity-before.json" "$proof_sha" "$core" "$server" "$source_web" "$target_image" || return
  native_dashboard_compose "$core_environment" "$target_environment" pull web || return
  sp_capacity || return
  native_dashboard_measure_capacity "$staging/capacity-after.json" "$proof_sha" "$core" "$server" "$source_web" "$target_image" || return
  sp_image "$target_image" "$target_image_id" || return
  markers=$(docker image inspect --format \
    '{{index .Config.Labels "org.weather.runtime.generation"}}|{{index .Config.Labels "org.opencontainers.image.revision"}}' \
    "$target_image") || return
  [[ "$markers" == "rust-v1|$target_commit" ]] || {
    native_dashboard_fail "target native image markers"
    return
  }
  native_dashboard_unchanged "$core_environment" "$protected" || return
  temporary="$staging/.stage.env.partial"
  umask 077
  printf '%s\n' \
    'NATIVE_DASHBOARD_CONTRACT=weather-native-dashboard-stage/v1' \
    "NATIVE_DASHBOARD_CORE_RELEASE=$core" \
    "NATIVE_DASHBOARD_SOURCE_RELEASE=$source" \
    "NATIVE_DASHBOARD_PUBLICATION_SHA256=$proof_sha" \
    "NATIVE_DASHBOARD_TARGET_ENV_SHA256=$(sp_sha "$target_environment")" \
    "NATIVE_DASHBOARD_PROTECTED_SHA256=$(sp_sha "$protected")" \
    "NATIVE_DASHBOARD_CAPACITY_BEFORE_SHA256=$(sp_sha "$staging/capacity-before.json")" \
    "NATIVE_DASHBOARD_CAPACITY_AFTER_SHA256=$(sp_sha "$staging/capacity-after.json")" >"$temporary" || return
  chmod 600 "$temporary" || return
  sync -f "$temporary" "$staging" || return
  mv -- "$temporary" "$staging/stage.env" || return
  sync -f "$staging" || return
  mv -T -- "$staging" "$directory" || return
  sync -f "$stage_root" || return
  trap - EXIT
  native_dashboard_stage_receipt "$target" >/dev/null || return
  printf 'Dashboard web release %s is staged over core %s.\n' "$target" "$core"
)

# remove the dashboard journal and its independently hashed protected snapshot
native_dashboard_close_journal() {
  local pending="$sp_root/native-dashboard/pending-protected.json"
  sp_private "$pending" || return
  remove_dashboard_state "$dashboard_release_journal" || return
  [[ ! -e "$dashboard_release_journal" && ! -L "$dashboard_release_journal" ]] || return 1
  sync -f "$state_dir" || return
  rm -- "$pending" || return
  sync -f "$sp_root/native-dashboard" "$state_dir"
}

# compare only the exact durable web marker tuple selected by the transaction
native_dashboard_verify_markers() {
  local target=$1 source=$2 core=$3 current previous active="$state_dir/active-web.env"
  current=$(read_optional_release_state "$state_dir/current-web-release") || return
  previous=$(read_optional_release_state "$state_dir/previous-web-release") || return
  if [[ "$target" == "$core" ]]; then
    [[ -z "$current" && -z "$previous" && ! -e "$active" && ! -L "$active" ]] || native_dashboard_fail "committed core markers"
  else
    [[ "$current" == "$target" && "$previous" == "$source" && -L "$active" &&
      "$(readlink "$active")" == "../releases/$target.web.env" ]] || native_dashboard_fail "committed web markers"
  fi
}

# verify restored marker writes even when an inherited helper returns after a partial failure
native_dashboard_restore_markers() {
  local original_current=$1 original_previous=$2 core
  restore_dashboard_marker_snapshot "$original_current" "$original_previous" || return
  core=$(read_release_state "$state_dir/current-release") || return
  if [[ "$original_current" == none ]]; then
    native_dashboard_verify_markers "$core" "$core" "$core" || return
  else
    native_dashboard_verify_markers "$original_current" "$original_previous" "$core" || return
  fi
  sync -f "$state_dir"
}

# switch only the web service under one recoverable marker transaction
native_dashboard_switch() (
  local action=$1 target=$2 core=$3 source=$4 target_environment=$5 proof_sha=$6
  local core_environment source_environment source_image source_image_id target_image target_image_id
  local original_current original_previous pending completed=false started=false status recovery_status boundary
  core_environment=$(sp_source) || return
  source_environment=$(native_dashboard_source_environment "$core" "$source") || return
  source_image=$(env_value "$source_environment" WEATHER_WEB_IMAGE)
  source_image_id=$(native_dashboard_environment_image_id "$core" "$source" "$source_environment") || return
  target_image=$(env_value "$target_environment" WEATHER_WEB_IMAGE)
  target_image_id=$(native_dashboard_environment_image_id "$core" "$target" "$target_environment") || return
  sp_database "$core_environment" || return
  [[ ! -e "$dashboard_release_journal" && ! -L "$dashboard_release_journal" ]] || {
    native_dashboard_fail "pending journal"
    return
  }
  pending="$sp_root/native-dashboard/pending-protected.json"
  # without a journal the prior snapshot never authorized a web mutation or already closed
  if [[ -e "$pending" || -L "$pending" ]]; then
    sp_private "$pending" || return
    rm -- "$pending" || return
    sync -f "$sp_root/native-dashboard" || return
  fi
  native_dashboard_snapshot "$core_environment" "$pending" || return
  sync -f "$pending" "$sp_root/native-dashboard" || return
  original_current=$(read_optional_release_state "$state_dir/current-web-release")
  original_previous=$(read_optional_release_state "$state_dir/previous-web-release")
  original_current=${original_current:-none}
  original_previous=${original_previous:-none}
  write_dashboard_release_journal "$action" "$core" "$source" "$target" \
    "$original_current" "$original_previous" "$source_environment" "$target_environment" \
    "$source_image" "$target_image" "$proof_sha" "$(sp_sha "$pending")" || return
  # restore the incumbent web and marker tuple after every incomplete switch
  # shellcheck disable=SC2329
  native_dashboard_switch_exit() {
    status=$?
    trap - EXIT
    # perform compensation only after the target start was attempted
    if [[ "$completed" != true ]]; then
      recovery_status=0
      if [[ "$started" == true ]]; then
        # classify durable markers before compensation so a fully committed target stays committed
        boundary=$(native_dashboard_recovery_boundary) || boundary=unknown
        if [[ "$boundary" == committed ]]; then
          native_dashboard_health "$core_environment" "$target_environment" "$target" "$target_image" "$target_image_id" || recovery_status=1
        elif [[ "$boundary" == original ]]; then
          # preserve every compensation failure instead of the status of only its last step
          native_dashboard_compose "$core_environment" "$source_environment" \
            up -d --no-deps --no-build --pull never --wait web || recovery_status=1
          native_dashboard_health "$core_environment" "$source_environment" \
            "$source" "$source_image" "$source_image_id" || recovery_status=1
          native_dashboard_restore_markers "$original_current" "$original_previous" || recovery_status=1
        else
          recovery_status=1
        fi
        native_dashboard_unchanged "$core_environment" "$pending" || recovery_status=1
      fi
      # retain a failed recovery journal for the separate recovery command
      if ((recovery_status == 0)); then
        native_dashboard_close_journal || recovery_status=$?
      fi
      ((recovery_status == 0)) || status=1
    fi
    exit "$status"
  }
  trap native_dashboard_switch_exit EXIT
  started=true
  native_dashboard_compose "$core_environment" "$target_environment" \
    up -d --no-deps --no-build --pull never --wait web || return
  native_dashboard_health "$core_environment" "$target_environment" \
    "$target" "$target_image" "$target_image_id" || return
  sp_database "$core_environment" || return
  native_dashboard_unchanged "$core_environment" "$pending" || return
  record_dashboard_release_success "$target" "$source" "$core" || return
  # recheck the actual committed link and markers before retiring the rollback journal
  native_dashboard_verify_markers "$target" "$source" "$core" || return
  completed=true
  native_dashboard_close_journal || return
  trap - EXIT
  printf 'Dashboard web release %s is active over core %s.\n' "$target" "$core"
)

# activate one exact previously staged native dashboard release
native_dashboard_activate() {
  local target=$1 coordinates directory core source protected publication proof_sha
  local target_environment core_environment target_image target_image_id current previous
  coordinates=$(native_dashboard_stage_receipt "$target") || return
  IFS=$'\t' read -r directory core source protected <<<"$coordinates"
  publication="$directory/publication.json"
  proof_sha=$(sp_sha "$publication")
  target_environment=$(dashboard_release_env "$target")
  validate_dashboard_release_env "$target_environment" "$target" "$core"
  current=$(read_optional_release_state "$state_dir/current-web-release")
  previous=$(read_optional_release_state "$state_dir/previous-web-release")
  # admit only the exact already-committed target as a completed retry
  if [[ "$current" == "$target" ]]; then
    [[ "$previous" == "$source" && -L "$state_dir/active-web.env" &&
      "$(readlink "$state_dir/active-web.env")" == "../releases/$target.web.env" ]] || {
      native_dashboard_fail "active retry markers"
      return
    }
    core_environment=$(sp_source) || return
    [[ "$(env_value "$core_environment" WEATHER_RELEASE)" == "$core" ]] || {
      native_dashboard_fail "active retry core"
      return
    }
    target_image=$(env_value "$target_environment" WEATHER_WEB_IMAGE)
    target_image_id=$(native_dashboard_environment_image_id "$core" "$target" "$target_environment") || return
    sp_database "$core_environment" || return
    native_dashboard_verify_committed_web "$core_environment" "$target_environment" \
      "$target" "$target_image" "$target_image_id" false || return
    printf 'Dashboard web release %s is already active over core %s.\n' "$target" "$core"
    return
  fi
  native_dashboard_live_admission "$publication" "$core" "$source" || return
  core_environment=$(sp_source) || return
  native_dashboard_activation_capacity "$directory" "$proof_sha" "$core" "$publication" "$core_environment" || return
  native_dashboard_switch release "$target" "$core" "$source" "$target_environment" "$proof_sha"
}

# stage and activate one fixed web-only publication
native_dashboard_release() {
  local target=$1 core=$2 source=$3
  native_dashboard_stage "$target" "$core" "$source" || return
  native_dashboard_activate "$target"
}

# bind the committed current and previous web markers to one reversible stage lineage edge
native_dashboard_admitted_pair() {
  local core=$1 current=$2 previous=$3 coordinates directory stage_core current_source protected previous_source
  [[ -n "$previous" && "$current" != "$core" && "$current" != "$previous" ]] || {
    native_dashboard_fail "dashboard marker ancestry"
    return
  }
  coordinates=$(native_dashboard_stage_receipt "$current") || return
  IFS=$'\t' read -r directory stage_core current_source protected <<<"$coordinates"
  [[ "$stage_core" == "$core" ]] || { native_dashboard_fail "current web stage core"; return; }
  # the first override names the unchanged core as its predecessor
  if [[ "$previous" == "$core" ]]; then
    [[ "$current_source" == "$core" ]] || native_dashboard_fail "core predecessor differs"
    return
  fi
  coordinates=$(native_dashboard_stage_receipt "$previous") || return
  IFS=$'\t' read -r directory stage_core previous_source protected <<<"$coordinates"
  # rollback reverses the same lineage edge and can subsequently be rolled forward
  [[ "$stage_core" == "$core" && ( "$current_source" == "$previous" || "$previous_source" == "$current" ) ]] ||
    native_dashboard_fail "previous web stage ancestry"
}

# return to the retained previous override or unchanged native core web
native_dashboard_rollback() {
  local mode=${1:-previous} core source target target_environment proof_sha
  local source_stage directory stage_core stage_source protected source_publication
  [[ "$mode" == previous || "$mode" == core ]] || {
    native_dashboard_fail "rollback mode"
    return
  }
  core=$(read_release_state "$state_dir/current-release")
  source=$(read_optional_release_state "$state_dir/current-web-release")
  [[ -n "$source" ]] || {
    native_dashboard_fail "no active override"
    return
  }
  source_stage=$(native_dashboard_stage_receipt "$source") || return
  IFS=$'\t' read -r directory stage_core stage_source protected <<<"$source_stage"
  [[ "$stage_core" == "$core" && "$stage_source" != "$source" ]] || {
    native_dashboard_fail "active stage ancestry"
    return
  }
  target=$(read_optional_release_state "$state_dir/previous-web-release")
  native_dashboard_admitted_pair "$core" "$source" "$target" || return
  # select the unchanged core only for the explicit core rollback
  if [[ "$mode" == core ]]; then
    target=$core
  fi
  [[ -n "$target" && "$target" != "$source" ]] || {
    native_dashboard_fail "rollback target"
    return
  }
  # bind either the core publication or one retained staged override
  if [[ "$target" == "$core" ]]; then
    target_environment=$(sp_source) || return
    source_publication="$sp_root/$core/publication.json"
    sp_private "$source_publication" || return
    proof_sha=$(sp_sha "$source_publication")
  else
    native_dashboard_stage_receipt "$target" >/dev/null || return
    target_environment=$(dashboard_release_env "$target")
    source_publication="$native_dashboard_publication_root/by-release/$target/publication.json"
    proof_sha=$(sp_sha "$source_publication")
  fi
  native_dashboard_switch rollback "$target" "$core" "$source" "$target_environment" "$proof_sha"
}

# classify only marker tuples reachable through the retained journal's write order
native_dashboard_recovery_boundary() {
  local core source target original_current original_previous current previous active
  local original_link target_link pending
  [[ -e "$dashboard_release_journal" && ! -L "$dashboard_release_journal" ]] || {
    native_dashboard_fail "missing recovery journal"
    return
  }
  validate_dashboard_release_journal || return
  pending="$sp_root/native-dashboard/pending-protected.json"
  sp_private "$pending" || return
  [[ "$(sp_sha "$pending")" == \
    "$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_SNAPSHOT_SHA256)" ]] || {
    native_dashboard_fail "recovery protected snapshot"
    return
  }
  core=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_RELEASE)
  source=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_RELEASE)
  target=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_RELEASE)
  original_current=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_ORIGINAL_CURRENT)
  original_previous=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_ORIGINAL_PREVIOUS)
  original_current=${original_current/none/}
  original_previous=${original_previous/none/}
  current=$(read_optional_release_state "$state_dir/current-web-release")
  previous=$(read_optional_release_state "$state_dir/previous-web-release")
  active=none
  [[ ! -e "$state_dir/active-web.env" || -L "$state_dir/active-web.env" ]] || {
    native_dashboard_fail "recovery active marker type"
    return
  }
  [[ ! -L "$state_dir/active-web.env" ]] || active=$(readlink "$state_dir/active-web.env")
  original_link=none
  [[ -z "$original_current" ]] || original_link="../releases/$original_current.web.env"
  target_link="../releases/$target.web.env"
  # recognize the completely committed non-core target
  if [[ "$target" != "$core" && "$current" == "$target" &&
    "$previous" == "$source" && "$active" == "$target_link" ]]; then
    printf 'committed\n'
    return
  fi
  # recognize the completely committed return to the unchanged core web
  if [[ "$target" == "$core" && -z "$current" && -z "$previous" && "$active" == none ]]; then
    printf 'committed\n'
    return
  fi
  # compensate the exact ordered link restoration after a physically committed marker write
  if [[ "$target" != "$core" && "$current" == "$target" && "$previous" == "$source" && "$active" == "$original_link" ]] ||
    [[ "$target" == "$core" && -z "$current" && -z "$previous" && "$active" == "$original_link" && "$original_link" != none ]]; then
    printf 'original\n'
    return
  fi
  [[ "$current" == "$original_current" ]] || {
    native_dashboard_fail "recovery current marker boundary"
    return
  }
  # admit only the three ordered marker states before a non-core current commit
  if [[ "$target" != "$core" ]] &&
    { [[ "$previous" == "$original_previous" && "$active" == "$original_link" ]] ||
      [[ "$previous" == "$source" && "$active" == "$original_link" ]] ||
      [[ "$previous" == "$source" && "$active" == "$target_link" ]]; }; then
    printf 'original\n'
    return
  fi
  # admit only the ordered link, previous and current removals during a core rollback
  if [[ "$target" == "$core" ]] &&
    { [[ "$previous" == "$original_previous" && "$active" == "$original_link" ]] ||
      [[ "$previous" == "$original_previous" && "$active" == none ]] ||
      [[ -z "$previous" && "$active" == "$original_link" ]] ||
      [[ -z "$previous" && "$active" == none ]]; }; then
    printf 'original\n'
    return
  fi
  native_dashboard_fail "recovery marker boundary"
}

# reconcile one exact interrupted web-only transaction without touching core services
native_dashboard_recover() {
  local boundary core source target original_current original_previous core_environment
  local source_environment target_environment source_image source_image_id target_image target_image_id
  local pending proof_sha target_publication target_publication_sha
  boundary=$(native_dashboard_recovery_boundary) || return
  pending="$sp_root/native-dashboard/pending-protected.json"
  core=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_RELEASE)
  source=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_RELEASE)
  target=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_RELEASE)
  original_current=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_ORIGINAL_CURRENT)
  original_previous=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_ORIGINAL_PREVIOUS)
  proof_sha=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_PROOF_SHA256)
  source_environment=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_ENV)
  target_environment=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_ENV)
  source_image=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_IMAGE)
  target_image=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_IMAGE)
  [[ "$source_image" == "$(env_value "$source_environment" WEATHER_WEB_IMAGE)" &&
    "$target_image" == "$(env_value "$target_environment" WEATHER_WEB_IMAGE)" ]] || {
    native_dashboard_fail "recovery web image environment"
    return
  }
  source_image_id=$(native_dashboard_environment_image_id "$core" "$source" "$source_environment") || return
  target_image_id=$(native_dashboard_environment_image_id "$core" "$target" "$target_environment") || return
  core_environment=$(sp_source) || return
  [[ "$(env_value "$core_environment" WEATHER_RELEASE)" == "$core" &&
    "$(sp_runtime_generation "$core_environment")" == rust-v1 ]] || {
    native_dashboard_fail "recovery core"
    return
  }
  sp_database "$core_environment" || return
  # reauthorize the exact target publication retained by the interrupted journal
  if [[ "$target" == "$core" ]]; then
    target_publication="$sp_root/$core/publication.json"
    sp_private "$target_publication" || return
    target_publication_sha=$(sp_sha "$target_publication")
  else
    native_dashboard_stage_receipt "$target" >/dev/null || return
    target_publication="$native_dashboard_publication_root/by-release/$target/publication.json"
    target_publication_sha=$(sp_sha "$target_publication")
  fi
  [[ "$target_publication_sha" == "$proof_sha" ]] || {
    native_dashboard_fail "recovery publication"
    return
  }
  # start only the journal-selected web after the core API and infrastructure are ready
  if [[ "$boundary" == committed ]]; then
    native_dashboard_compose "$core_environment" "$target_environment" \
      up -d --no-deps --no-build --pull never --wait web || return
    native_dashboard_health "$core_environment" "$target_environment" \
      "$target" "$target_image" "$target_image_id" || return
  else
    native_dashboard_compose "$core_environment" "$source_environment" \
      up -d --no-deps --no-build --pull never --wait web || return
    native_dashboard_health "$core_environment" "$source_environment" \
      "$source" "$source_image" "$source_image_id" || return
    native_dashboard_restore_markers "$original_current" "$original_previous" || return
  fi
  native_dashboard_unchanged "$core_environment" "$pending" || return
  native_dashboard_close_journal
}

# start the exact committed dashboard web or reconcile its retained transaction after core startup
native_dashboard_system_recover() {
  (($# == 0)) || {
    native_dashboard_fail "system recovery operands"
    return
  }
  local core_environment core current coordinates directory stage_core stage_source protected
  local web_environment web_image web_image_id
  # reconcile the journal only after the caller has restored the core API and infrastructure
  if [[ -e "$dashboard_release_journal" || -L "$dashboard_release_journal" ]]; then
    native_dashboard_recover
    return
  fi
  native_dashboard_context_admission --recovery || return
  core_environment=$(sp_source) || return
  core=$(env_value "$core_environment" WEATHER_RELEASE)
  current=$(read_optional_release_state "$state_dir/current-web-release")
  # restore the unchanged core web when no committed dashboard override exists
  if [[ -z "$current" ]]; then
    web_image=$(env_value "$core_environment" WEATHER_WEB_IMAGE)
    web_image_id=$(native_dashboard_environment_image_id "$core" "$core" "$core_environment") || return
    sp_database "$core_environment" || return
    native_dashboard_verify_committed_web "$core_environment" "$core_environment" \
      "$core" "$web_image" "$web_image_id" true
    return
  fi
  coordinates=$(native_dashboard_stage_receipt "$current") || return
  IFS=$'\t' read -r directory stage_core stage_source protected <<<"$coordinates"
  [[ "$stage_core" == "$core" ]] || {
    native_dashboard_fail "recovery active stage core"
    return
  }
  web_environment=$(dashboard_release_env "$current")
  validate_dashboard_release_env "$web_environment" "$current" "$core" || return
  web_image=$(env_value "$web_environment" WEATHER_WEB_IMAGE)
  web_image_id=$(native_dashboard_environment_image_id "$core" "$current" "$web_environment") || return
  sp_database "$core_environment" || return
  native_dashboard_verify_committed_web "$core_environment" "$web_environment" \
    "$current" "$web_image" "$web_image_id" true
}

# admit only complete ordinary markers or exact journal states in recovery mode
native_dashboard_context_admission() {
  local mode=${1:-normal} current previous active core
  [[ "$mode" == normal || "$mode" == --recovery ]] || {
    native_dashboard_fail "context mode"
    return
  }
  # normal mutations never inherit an interrupted dashboard transaction
  if [[ -e "$dashboard_release_journal" || -L "$dashboard_release_journal" ]]; then
    [[ "$mode" == --recovery ]] || {
      native_dashboard_fail "pending recovery journal"
      return
    }
    native_dashboard_recovery_boundary >/dev/null
    return
  fi
  current=$(read_optional_release_state "$state_dir/current-web-release")
  previous=$(read_optional_release_state "$state_dir/previous-web-release")
  active="$state_dir/active-web.env"
  # require the complete inactive tuple when no override exists
  if [[ -z "$current" ]]; then
    [[ -z "$previous" && ! -e "$active" && ! -L "$active" ]] ||
      native_dashboard_fail "inactive marker tuple"
    return
  fi
  validate_release "$current"
  # require the retained rollback marker for every active override
  if [[ -z "$previous" ]] || ! validate_release "$previous"; then
    native_dashboard_fail "previous web marker"
    return
  fi
  [[ -L "$active" && "$(readlink "$active")" == "../releases/$current.web.env" ]] ||
    native_dashboard_fail "active marker tuple"
  core=$(read_release_state "$state_dir/current-release")
  validate_dashboard_release_env "$(dashboard_release_env "$current")" "$current" "$core" || return
  native_dashboard_admitted_pair "$core" "$current" "$previous"
}
