#!/usr/bin/env bash

readonly native_release_publication_root=/opt/weather/operators/native-publications

# classify the complete forced-command release-control surface before legacy fallback.
native_release_control_handles() {
  local action=${1-}
  # keep every release mutation on a generation-aware path.
  case "$action" in
    rollback|recover|status|preflight|stage|activate|yolo|dashboard-release|dashboard-rollback|dashboard-rollback-core)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# report malformed release-control operands without entering an owning operation.
native_release_control_invalid_arguments() {
  printf 'error: invalid native release-control arguments\n' >&2
  return 2
}

# report fixed publication admission failures without selecting legacy release code
native_release_control_fail() {
  printf 'error: native release-control publication differs: %s\n' "$1" >&2
  return 1
}

# validate one immutable Weather release label before selecting its native owner.
native_release_control_validate_release() {
  [[ ${1-} =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]?$ ]] || return 1
  validate_release "$1"
}

# resolve one independently installed root-owned descendant publication
native_release_control_publication() {
  local release=$1 base directory publication checksum digest line checksum_bytes path
  base="$native_release_publication_root/by-release"
  directory="$base/$release"
  publication="$directory/publication.json"
  checksum="$directory/publication.sha256"
  # bind every fixed directory without following an alternate namespace
  for path in "$native_release_publication_root" "$base" "$directory"; do
    [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" &&
      "$(stat -c '%u:%g:%a' -- "$path")" == 0:0:700 ]] || {
      native_release_control_fail "fixed publication directory"
      return
    }
  done
  sp_private "$publication" || return
  sp_private "$checksum" || return
  digest=$(sp_sha "$publication")
  line=$(cat -- "$checksum")
  checksum_bytes=$(wc -c <"$checksum")
  # require the exact checksum-file grammar and installed validator acceptance
  [[ "$checksum_bytes" == 83 && "$line" == "$digest  publication.json" ]] || {
    native_release_control_fail "publication checksum"
    return
  }
  [[ "$(sp_field "$publication" contractVersion)" == \
    weather-schema-preserving-native-descendant-publication/v1 ]] || {
    native_release_control_fail "ordinary descendant contract"
    return
  }
  # shellcheck disable=SC2154
  node "$sp_module" validate-publication "$publication" "$release" >/dev/null || {
    native_release_control_fail "publication contract"
    return
  }
  printf '%s\t%s\n' "$publication" "$digest"
}

# stage one exact descendant or admit only its identical retained retry
native_release_control_stage() {
  local release=$1 coordinates publication digest retained
  coordinates=$(native_release_control_publication "$release") || return
  IFS=$'\t' read -r publication digest <<<"$coordinates"
  # shellcheck disable=SC2154
  retained="$sp_root/$release/stage.json"
  # preserve an already completed identical stage without pulling its images again
  if [[ -e "$retained" || -L "$retained" ]]; then
    sp_target "$release" >/dev/null || return
    sp_private "$sp_root/$release/publication.json" || return
    [[ "$(sp_sha "$sp_root/$release/publication.json")" == "$digest" ]] || {
      native_release_control_fail "retained stage publication"
      return
    }
    printf 'Application release %s is already staged.\n' "$release"
    return
  fi
  sp_stage "$release" "$publication" "$digest"
}

# stage and activate one exact descendant while retaining completed retry semantics
native_release_control_yolo() {
  local release=$1 source
  source=$(sp_source) || return
  # treat only an independently healthy exact active release as completed
  if [[ "$(env_value "$source" WEATHER_RELEASE)" == "$release" ]]; then
    sp_health "$source" || return
    printf 'Application release %s is already active.\n' "$release"
    return
  fi
  native_release_control_stage "$release" || return
  sp_activate "$release"
}

# retain the original full host status after the auxiliary controller proves ancestry.
native_release_control_status() {
  (($# == 0)) || {
    native_release_control_invalid_arguments
    return
  }
  sp_source >/dev/null || return
  # shellcheck disable=SC2154
  "$deploy_dir/scripts/status.sh"
}

# retain the original fixed resource sample without entering an application image.
native_release_control_preflight() {
  (($# == 0)) || {
    native_release_control_invalid_arguments
    return
  }
  "$deploy_dir/scripts/preflight-capacity.sh" --sample-seconds 900 \
    --json /var/lib/weather/preflight-v13-resource.json
}

# dispatch only operations with an exact native owner and close every unsafe fallback.
native_release_control_dispatch() {
  (($# >= 1)) || {
    native_release_control_invalid_arguments
    return
  }
  local action=$1
  shift
  # preserve each forced command's existing operand grammar.
  case "$action" in
    rollback)
      (($# == 0)) || {
        native_release_control_invalid_arguments
        return
      }
      sp_rollback
      ;;
    recover)
      (($# == 0)) || {
        native_release_control_invalid_arguments
        return
      }
      sp_system_recover
      ;;
    status)
      native_release_control_status "$@"
      ;;
    preflight)
      native_release_control_preflight "$@"
      ;;
    activate)
      # require the exact existing public release operand.
      if (($# != 1)) || ! native_release_control_validate_release "$1"; then
        native_release_control_invalid_arguments
        return
      fi
      sp_activate "$1"
      ;;
    stage)
      # reject malformed releases before opening the fixed publication namespace.
      if (($# != 1)) || ! native_release_control_validate_release "$1"; then
        native_release_control_invalid_arguments
        return
      fi
      native_release_control_stage "$1"
      ;;
    yolo)
      # reject malformed releases before opening the fixed publication namespace.
      if (($# != 1)) || ! native_release_control_validate_release "$1"; then
        native_release_control_invalid_arguments
        return
      fi
      native_release_control_yolo "$1"
      ;;
    dashboard-release)
      # preserve the three-release dashboard command grammar before selecting its owner
      if (($# != 3)) || ! native_release_control_validate_release "$1" ||
        ! native_release_control_validate_release "$2" ||
        ! native_release_control_validate_release "$3"; then
        native_release_control_invalid_arguments
        return
      fi
      native_dashboard_release "$1" "$2" "$3"
      ;;
    dashboard-rollback)
      (($# == 0)) || {
        native_release_control_invalid_arguments
        return
      }
      native_dashboard_rollback previous
      ;;
    dashboard-rollback-core)
      (($# == 0)) || {
        native_release_control_invalid_arguments
        return
      }
      native_dashboard_rollback core
      ;;
    *)
      return 64
      ;;
  esac
}
