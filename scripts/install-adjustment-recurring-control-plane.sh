#!/usr/bin/env bash
set -euo pipefail

readonly maintenance_previous_release=2026.10.09-1
readonly maintenance_recorded_control_plane_sha256=603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba
readonly maintenance_installed_predecessor_control_plane_sha256=08b25ce2c80baadbc3f94c8e57a694387853c3a9bab06364c7e7906e5e04de40
readonly maintenance_backup_max_bytes=$((4 * 1024 * 1024 * 1024))
readonly maintenance_backup_overhead_bytes=$((1024 * 1024))
readonly -a maintenance_changed_paths=(
  deploy/postgres/runtime-acl-v2.sql
  deploy/scripts/adjustment-evaluation-package.mjs
  deploy/scripts/adjustment-evidence-store.mjs
  deploy/scripts/backup.sh
  deploy/scripts/common.sh
  deploy/scripts/public-stations-backfill.sh
  deploy/scripts/tempest-backfill.sh
  deploy/scripts/tide-backfill.sh
  deploy/scripts/update.sh
  deploy/scripts/web-server.mjs
)

# print one fatal installer error
fail() {
  printf 'error: %s\n' "$*" >&2
  return 1
}

# emit the frozen forty-two predecessor identities
predecessor_identity_manifest() {
  cat <<'EOF'
0fa4b2b990345a110ab324e13bf2e3528c19d530bf9a4d2b8576d3b42eebeade 0 0 0644 deploy/compose.yaml
c598784b280e13b9f12982e93df751b1b9a4290bb3c7a5c1994808cb75e16223 0 0 0775 deploy/postgres/010-create-runtime-roles.sh
20f90bfd9f86f9c35a8c0fa540760c71a96e0a9d1d2a71049ca0fbcf15a3031f 0 0 0775 deploy/postgres/postgres-admin-entrypoint.sh
b1e1f92d24a84317797bbe1ead5b877d3335e1ccadc54cb2680a55d033a24cc5 0 0 0664 deploy/postgres/runtime-acl-v1.sql
fd277fe573c737da78bbb3b06432530597fbc0472f6e8212a56e045b0409c474 0 0 0644 deploy/postgres/runtime-acl-v2.sql
09a456e821cf39d03a257f4795a05fd9787e10893869c455df7ca78bd2d7919a 0 0 0755 deploy/scripts/adjustment-evaluation-export.sh
8cbe75eb781cf0886513e24d1c3fcfa582202897314ac4d1aea4d30071502226 0 0 0644 deploy/scripts/adjustment-evaluation-package.mjs
1570c6154682fc1f2a7619e9a229a2477da32b5316d607cdedbf70f64591b332 0 0 0644 deploy/scripts/adjustment-evidence-store.mjs
3446f5d8a3840bf4caf57ee7c09bdfccaa546d69abb6f31888d6a51192273038 0 0 0775 deploy/scripts/backup-stream.sh
cb862c9ff824c1558f8475e42f51ae658ba94ddc8b8db5ed0fec47abd236733a 0 0 0775 deploy/scripts/backup.sh
00b98a09ee8e44672acb50517e0cab95766956c73d57d587001575c10fb1cd2a 0 0 0755 deploy/scripts/common.sh
8b7b4b1fa44ad2969066487ced0269b9d594cba4b3c29d17d6bffb474e16a6e3 0 0 0664 deploy/scripts/compatibility-provider.mjs
50c70b59285d2dd10e5e63a5c04b7331cfadd1da7e3db872d8c5561839e54975 0 0 0644 deploy/scripts/forecast-adjustment-scorecard-contract.mjs
48db6635659da7dba0062594f9ae1146176c40d22a714a58b7675b3f1570b5f8 0 0 0775 deploy/scripts/forecast-training-export.sh
f1bd65685da06f0d6969ad9b96722c5edb6f030e291a375bfb3e36192db76787 0 0 0755 deploy/scripts/forecast-training-package.mjs
3dc0cf6b07f4a844f95fffd58352523636d7b93c4b51f42ad47e1161c7dd0266 0 0 0644 deploy/scripts/home-network.mjs
6cc70ddf1ad123c0ba4863405f0fe7b83b124ad5ff69fd126a295f354b6a4226 0 0 0755 deploy/scripts/install-adjustment-scorecard.sh
4b56fc50489d90c64271d129fb0bda63968eacfd54201e8ae7985c0df5d6227d 0 0 0664 deploy/scripts/migrate.mjs
e8e70038bf77a85ca5eafc8047800ebbd931edd81d5f58a0228908ea4858987e 0 0 0775 deploy/scripts/preflight-capacity.sh
f57d8568f87040821c32274d1fa84b3058aba353ee03447b95dbcdfaf6191e57 0 0 0775 deploy/scripts/public-stations-backfill.sh
9fda1ec3e3a4a6d9522e6a00c478af406b2767b20b9e971cdb9d7a4cf0bfae8b 0 0 0755 deploy/scripts/publish-adjustment-scorecard.sh
72875b48e703da00b9683c9f6a86506e0bcfe9f492f4c4a5275ee69590f0dd1d 0 0 0755 deploy/scripts/pull-adjustment-evaluation-export.sh
566e114a050d54743e679f2ba5b1c39a7466a24dc8040fed910dff30da72355e 0 0 0775 deploy/scripts/pull-backup.sh
1cd1f32732bb30837f86e3fdab10d31ebb06e3bace2de1e219aed8ee4b320053 0 0 0775 deploy/scripts/pull-forecast-training-export.sh
7a0bc4c09cb1044f5e3a3123319a819138a7d029158e07163673f80c18331496 0 0 0755 deploy/scripts/remote-ops.sh
f4f845fd094ac39c5ab1533efff0a4d5acd5e820a2916e625fbd7532dd070d22 0 0 0664 deploy/scripts/resolve-image.mjs
106957ec33fe86dfcef039129a2890df8655cf8521ee80cd4cf1282fefa9e9a9 0 0 0775 deploy/scripts/restore.sh
0b8bf4c0e65a9a1f1f272c519ba4c7c098f5367ccaf30f47304fe966c9f29dd5 0 0 0755 deploy/scripts/ssh-dispatch.sh
f568eec0fb0c16dfe9096dd6a54780c3aa07ea39af0318b08fcea68c82b16fd9 0 0 0755 deploy/scripts/ssh-run.sh
a18d836d4646634d31bfd0fa3f7d5e73264f299391c958763151c6f0c0ed2b86 0 0 0775 deploy/scripts/status.sh
8218aae7bb2f7af21348e4d8e3a6d5796a3d52279ab70607c61f362a5ef2090f 0 0 0775 deploy/scripts/tempest-backfill.sh
3709e03ada7f1864b49f50e880a81f1b3ae23bd903d6c9b0109d9b15d553e5fa 0 0 0775 deploy/scripts/tide-backfill.sh
36680cbe7d3c0af638a276817db071727dc47e353a3ad9c4f6362f3941b2353d 0 0 0755 deploy/scripts/update.sh
aadfcdfa9fbdf76761616675e8c52995840b5bf6489185e4ba8a8eb9589e4dad 0 0 0775 deploy/scripts/verify-static.sh
27ac1c81f2d178d1d36e5b350f294ce901873e35876ad5e128422141d7eab0cd 0 0 0644 deploy/scripts/weather-admin-store.mjs
b6e19b27c63a7a9cc1808e0a0ca97423d357af21904bf090152b7ce95f774fca 0 0 0644 deploy/scripts/web-server.mjs
2a0006cf85e7b6416c4628bcef23ddd55553402f63382358b283e2f8c37ffa82 0 0 0664 deploy/scripts/xweather-tile-cache.mjs
2a2f3e0cc8f59369a5235454a4237eed91c60cdbe374dcd1c8c7821731e2e3d0 0 0 0664 deploy/scripts/xweather-usage-budget.mjs
4f8b4d58b8645f40e5da779857669114a2bec11cb720aedd7be340c3c6c1ac00 0 0 0664 deploy/sudoers/weather-ops
85d77f2e61be5dab2d387e867a8dfb4ff70a90e659c5e9e91d633858d0c3d4aa 0 0 0664 deploy/systemd/weather-backup-local.service
9fb0866ceb3b81f90e0c579628a8028bc807d3f7c1a7ac9ff3e37c505c2d02dd 0 0 0664 deploy/systemd/weather-backup-local.timer
12e33019380f77215a44340e4d6feb4a0dc13f76c968ab36c4501d039ec24de8 0 0 0664 deploy/systemd/weather-compose.service
EOF
}

# emit the reviewed changed paths in installation order
maintenance_changed_path_manifest() {
  printf '%s\n' "${maintenance_changed_paths[@]}"
}

# hash the exact forty-two-file control surface
control_digest() {
  local root=$1
  local relative
  (
    # retain the deployed digest order with compose appended last
    while IFS= read -r relative; do
      printf '%s\0' "$relative"
      sha256sum "$root/$relative" | awk '{print $1}'
    done < <(
      expected_control_paths | grep -Fvx deploy/compose.yaml | LC_ALL=C sort
      printf '%s\n' deploy/compose.yaml
    )
  ) | sha256sum | awk '{print $1}'
}

# emit the expected closed control paths
expected_control_paths() {
  local relative
  # strip identities to one sorted path list
  while read -r _ _ _ _ relative; do
    printf '%s\n' "$relative"
  done < <(predecessor_identity_manifest)
}

# verify one exact closed control surface
verify_closed_control_paths() {
  local root=$1
  local entry relative
  local -A expected=()
  [[ -d "$root/deploy/scripts" && ! -L "$root/deploy/scripts" ]] ||
    fail "control scripts directory is missing or linked"
  [[ -d "$root/deploy/postgres" && ! -L "$root/deploy/postgres" ]] ||
    fail "control postgres directory is missing or linked"
  [[ -d "$root/deploy/systemd" && ! -L "$root/deploy/systemd" ]] ||
    fail "control systemd directory is missing or linked"
  [[ -d "$root/deploy/sudoers" && ! -L "$root/deploy/sudoers" ]] ||
    fail "control sudoers directory is missing or linked"
  # index the exact regular-file surface
  while IFS= read -r relative; do
    expected["$relative"]=1
  done < <(expected_control_paths)
  # reject every hidden link, node or nested directory
  while IFS= read -r -d '' entry; do
    relative=${entry#"$root"/}
    [[ -n "${expected[$relative]:-}" && -f "$entry" && ! -L "$entry" ]] ||
      fail "control surface contains an unreviewed entry: $relative"
    unset "expected[$relative]"
  done < <(find -P "$root/deploy/scripts" "$root/deploy/postgres" \
    "$root/deploy/systemd" "$root/deploy/sudoers" -mindepth 1 -print0)
  [[ -f "$root/deploy/compose.yaml" && ! -L "$root/deploy/compose.yaml" ]] ||
    fail "control compose path is missing or linked"
  unset 'expected[deploy/compose.yaml]'
  ((${#expected[@]} == 0)) ||
    fail "control surface is missing a reviewed path"
}

# verify one canonical directory identity
require_owned_directory() {
  local path=$1
  local uid=$2
  local gid=$3
  local label=$4
  [[ -d "$path" && ! -L "$path" && "$(realpath -e "$path")" == "$path" ]] ||
    fail "$label is missing, linked or noncanonical"
  [[ "$(stat -c '%u' "$path")" == "$uid" &&
    "$(stat -c '%g' "$path")" == "$gid" ]] ||
    fail "$label owner differs"
}

# verify one control target's fixed ancestors
verify_control_target_ancestry() {
  local destination=$1
  local relative=$2
  local expected_uid=$3
  local expected_gid=$4
  require_owned_directory "$destination" "$expected_uid" "$expected_gid" \
    "deployment root"
  require_owned_directory "$destination/deploy" "$expected_uid" "$expected_gid" \
    "deployment control root"
  require_owned_directory "$destination/$(dirname "$relative")" \
    "$expected_uid" "$expected_gid" "control target parent"
}

# verify all fixed deployment and runtime ancestors
verify_fixed_ancestry() {
  local destination=$1
  local runtime_root=$2
  local control_uid=$3
  local control_gid=$4
  local data_uid=$5
  local data_gid=$6
  local directory
  # verify every root-controlled deployment directory
  for directory in deploy deploy/scripts deploy/postgres deploy/systemd \
    deploy/sudoers deploy/state deploy/releases; do
    require_owned_directory "$destination/$directory" "$control_uid" "$control_gid" \
      "deployment ancestor $directory"
  done
  require_owned_directory "$destination" "$control_uid" "$control_gid" \
    "deployment root"
  require_owned_directory "$runtime_root" "$control_uid" "$control_gid" \
    "runtime root"
  require_owned_directory "$runtime_root/xweather" "$data_uid" "$data_gid" \
    "xweather state parent"
  # verify present service-owned evidence state
  if [[ -e "$runtime_root/xweather/adjustment-evidence" ||
    -L "$runtime_root/xweather/adjustment-evidence" ]]; then
    require_owned_directory "$runtime_root/xweather/adjustment-evidence" \
      "$data_uid" "$data_gid" "adjustment evidence state root"
  fi
  # verify present root-owned release state
  if [[ -e "$runtime_root/xweather/adjustment-release-state" ||
    -L "$runtime_root/xweather/adjustment-release-state" ]]; then
    require_owned_directory "$runtime_root/xweather/adjustment-release-state" \
      "$control_uid" "$control_gid" "adjustment release state root"
  fi
}

# verify the frozen predecessor identities
verify_predecessor_identities() {
  local root=$1
  local expected_uid=${2:-0}
  local expected_gid=${3:-0}
  local sha uid gid mode relative actual_sha actual_uid actual_gid actual_mode
  verify_closed_control_paths "$root"
  # compare every content, owner and mode identity
  while read -r sha uid gid mode relative; do
    [[ "$uid" == 0 && "$gid" == 0 ]] || fail "embedded predecessor owner is invalid"
    [[ -f "$root/$relative" && ! -L "$root/$relative" ]] ||
      fail "predecessor path is missing or linked: $relative"
    actual_sha=$(sha256sum "$root/$relative" | awk '{print $1}')
    actual_uid=$(stat -c '%u' "$root/$relative")
    actual_gid=$(stat -c '%g' "$root/$relative")
    printf -v actual_mode '%04o' "$((8#$(stat -c '%a' "$root/$relative")))"
    [[ "$actual_sha" == "$sha" && "$actual_uid" == "$expected_uid" &&
      "$actual_gid" == "$expected_gid" && "$actual_mode" == "$mode" ]] ||
      fail "predecessor identity differs: $relative"
  done < <(predecessor_identity_manifest)
  [[ "$(control_digest "$root")" == "$maintenance_installed_predecessor_control_plane_sha256" ]] ||
    fail "predecessor control digest differs"
}

# require the one reviewed predecessor release record
require_maintenance_predecessor_release() {
  local destination=$1
  local release_env="$destination/deploy/releases/$maintenance_previous_release.env"
  [[ -f "$destination/deploy/state/current-release" &&
    ! -L "$destination/deploy/state/current-release" ]] ||
    fail "installed release state is missing or linked"
  [[ "$(cat "$destination/deploy/state/current-release")" == "$maintenance_previous_release" ]] ||
    fail "installed release is not the reviewed maintenance predecessor"
  [[ -f "$release_env" && ! -L "$release_env" ]] ||
    fail "maintenance predecessor metadata is missing or linked"
  grep -Fxq "WEATHER_RELEASE=$maintenance_previous_release" "$release_env" ||
    fail "maintenance predecessor release identity differs"
  grep -Fxq 'WEATHER_CONTROL_PLANE_VERSION=13' "$release_env" ||
    fail "maintenance predecessor control version differs"
  grep -Fxq "WEATHER_CONTROL_PLANE_SHA256=$maintenance_recorded_control_plane_sha256" \
    "$release_env" || fail "maintenance predecessor control digest differs"
}

# reject active release, backup or migration operations
require_quiet_weather() {
  local command_line process
  # inspect lifecycle commands without claiming a global application lock
  for process in /proc/[0-9]*/cmdline; do
    [[ -r "$process" ]] || continue
    command_line=$(tr '\0' ' ' <"$process" 2>/dev/null) || continue
    # reject every overlapping privileged lifecycle command
    if [[ "$command_line" == *'/opt/weather/current/deploy/scripts/update.sh'* ||
      "$command_line" == *'/opt/weather/current/deploy/scripts/restore.sh'* ||
      "$command_line" == *'/opt/weather/current/deploy/scripts/backup.sh'* ||
      "$command_line" == *'/opt/weather/current/deploy/scripts/install-adjustment-recurring-control-plane.sh'* ]]; then
      fail "a Weather lifecycle operation is in flight"
    fi
  done
  [[ -z "$(docker ps --quiet \
    --filter label=com.docker.compose.project=weather \
    --filter label=com.docker.compose.service=migration 2>/dev/null)" ]] ||
    fail "a Weather migration container is in flight"
}

# classify one reviewed changed path
is_maintenance_changed_path() {
  local requested=$1
  local relative
  # match only the fixed ten-path list
  for relative in "${maintenance_changed_paths[@]}"; do
    [[ "$requested" != "$relative" ]] || return 0
  done
  return 1
}

# validate one archive entry name
is_control_archive_entry() {
  local entry=${1%/}
  local relative
  # allow only fixed parent directories
  case "$entry" in
    deploy|deploy/scripts|deploy/postgres|deploy/systemd|deploy/sudoers) return 0 ;;
  esac
  # allow only exact frozen files
  while read -r _ _ _ _ relative; do
    [[ "$entry" != "$relative" ]] || return 0
  done < <(predecessor_identity_manifest)
  return 1
}

# validate and extract one closed candidate archive
extract_candidate_archive() {
  local archive=$1
  local stage=$2
  local entry line
  local -A seen=()
  tar -tf "$archive" >"$stage/archive-paths"
  # reject traversal, duplicates and additions
  while IFS= read -r entry; do
    [[ "$entry" =~ ^deploy(/([a-zA-Z0-9._-]+))*/?$ &&
      ! "$entry" =~ (^|/)\.\.(/|$) && ! "$entry" =~ (^|/)\.(/|$) ]] ||
      fail "candidate archive contains a forbidden path"
    is_control_archive_entry "$entry" ||
      fail "candidate archive changes the closed control surface: $entry"
    entry=${entry%/}
    [[ -z "${seen[$entry]:-}" ]] ||
      fail "candidate archive repeats a control entry: $entry"
    seen[$entry]=1
  done <"$stage/archive-paths"
  tar -tvf "$archive" >"$stage/archive-types"
  # reject links and device-like entries
  while IFS= read -r line; do
    [[ "${line:0:1}" == - || "${line:0:1}" == d ]] ||
      fail "candidate archive contains a non-file entry"
  done <"$stage/archive-types"
  mkdir "$stage/candidate"
  tar --no-same-owner --no-same-permissions -xf "$archive" -C "$stage/candidate"
  verify_closed_control_paths "$stage/candidate"
}

# compute one desired candidate identity manifest
candidate_identity_manifest() {
  local candidate=$1
  local expected_uid=$2
  local expected_gid=$3
  local _sha _uid _gid mode relative
  # retain predecessor modes and bind new content hashes
  while read -r _sha _uid _gid mode relative; do
    printf '%s %s %s %s %s\n' \
      "$(sha256sum "$candidate/$relative" | awk '{print $1}')" \
      "$expected_uid" "$expected_gid" "$mode" "$relative"
  done < <(predecessor_identity_manifest)
}

# hash one identity manifest using the deployed digest order
control_digest_from_identity_manifest() {
  local manifest=$1
  local relative sha
  (
    # bind each required path to its declared hash
    while IFS= read -r relative; do
      sha=$(awk -v path="$relative" '$5 == path { print $1 }' "$manifest")
      [[ "$sha" =~ ^[a-f0-9]{64}$ ]] ||
        fail "candidate identity manifest is missing: $relative"
      printf '%s\0%s\n' "$relative" "$sha"
    done < <(
      expected_control_paths | grep -Fvx deploy/compose.yaml | LC_ALL=C sort
      printf '%s\n' deploy/compose.yaml
    )
  ) | sha256sum | awk '{print $1}'
}

# verify one sealed candidate identity manifest
verify_candidate_identity_manifest_file() {
  local manifest=$1
  local candidate_digest=$2
  local expected_uid=$3
  local expected_gid=$4
  local sha uid gid mode relative old_sha changed_count=0 line_count=0
  local -A expected=() changed=()
  [[ -f "$manifest" && ! -L "$manifest" ]] ||
    fail "sealed candidate identity manifest is missing or linked"
  # index the frozen predecessor hashes
  while read -r old_sha _ _ _ relative; do
    expected["$relative"]=$old_sha
  done < <(predecessor_identity_manifest)
  # verify every exact candidate member
  while read -r sha uid gid mode relative; do
    line_count=$((line_count + 1))
    [[ "$sha" =~ ^[a-f0-9]{64}$ && "$uid" == "$expected_uid" &&
      "$gid" == "$expected_gid" && "$mode" =~ ^0[0-7]{3}$ &&
      -n "${expected[$relative]:-}" ]] ||
      fail "sealed candidate identity is invalid: $relative"
    [[ "$mode" == "$(awk -v path="$relative" '$5 == path { print $4 }' \
      <(predecessor_identity_manifest))" ]] ||
      fail "sealed candidate mode differs: $relative"
    old_sha=${expected[$relative]}
    unset "expected[$relative]"
    # classify each declared byte change
    if [[ "$sha" != "$old_sha" ]]; then
      is_maintenance_changed_path "$relative" ||
        fail "sealed candidate changes an unreviewed path: $relative"
      changed["$relative"]=1
      changed_count=$((changed_count + 1))
    fi
  done <"$manifest"
  [[ "$line_count" == 42 && "${#expected[@]}" == 0 &&
    "$changed_count" == "${#maintenance_changed_paths[@]}" ]] ||
    fail "sealed candidate identity manifest is incomplete"
  # require all reviewed changes
  for relative in "${maintenance_changed_paths[@]}"; do
    [[ -n "${changed[$relative]:-}" ]] ||
      fail "sealed candidate leaves a required path unchanged: $relative"
  done
  [[ "$(control_digest_from_identity_manifest "$manifest")" == "$candidate_digest" ]] ||
    fail "sealed candidate control digest differs"
}

# emit one actual identity manifest
actual_identity_manifest() {
  local root=$1
  local _sha _uid _gid _mode relative
  # bind each frozen path to content, owner and mode
  while read -r _sha _uid _gid _mode relative; do
    printf '%s %s %s %04o %s\n' \
      "$(sha256sum "$root/$relative" | awk '{print $1}')" \
      "$(stat -c '%u' "$root/$relative")" \
      "$(stat -c '%g' "$root/$relative")" \
      "$((8#$(stat -c '%a' "$root/$relative")))" "$relative"
  done < <(predecessor_identity_manifest)
}

# require exactly the ten reviewed candidate changes
verify_candidate_identities() {
  local candidate=$1
  local candidate_digest=$2
  local old_sha _uid _gid _mode relative new_sha changed_count=0
  local -A changed=()
  verify_closed_control_paths "$candidate"
  # classify every changed content identity
  while read -r old_sha _uid _gid _mode relative; do
    [[ -f "$candidate/$relative" && ! -L "$candidate/$relative" ]] ||
      fail "candidate path is missing or linked: $relative"
    new_sha=$(sha256sum "$candidate/$relative" | awk '{print $1}')
    # record only actual byte changes
    if [[ "$new_sha" != "$old_sha" ]]; then
      is_maintenance_changed_path "$relative" ||
        fail "candidate changes an unreviewed path: $relative"
      changed["$relative"]=1
      changed_count=$((changed_count + 1))
    fi
  done < <(predecessor_identity_manifest)
  [[ "$changed_count" == "${#maintenance_changed_paths[@]}" ]] ||
    fail "candidate must change exactly ${#maintenance_changed_paths[@]} reviewed paths; found $changed_count"
  # require every reviewed path to change
  for relative in "${maintenance_changed_paths[@]}"; do
    [[ -n "${changed["$relative"]:-}" ]] ||
      fail "candidate leaves a required path unchanged: $relative"
  done
  grep -Fxq 'control_plane_version=15' "$candidate/deploy/scripts/update.sh" ||
    fail "candidate control version is not fifteen"
  grep -Fxq "recurring_previous_release=$maintenance_previous_release" \
    "$candidate/deploy/scripts/update.sh" ||
    fail "candidate does not pin the maintenance predecessor release"
  grep -Fxq "recurring_previous_control_plane_sha256=$maintenance_recorded_control_plane_sha256" \
    "$candidate/deploy/scripts/update.sh" ||
    fail "candidate does not pin the maintenance predecessor digest"
  [[ "$(control_digest "$candidate")" == "$candidate_digest" ]] ||
    fail "candidate control digest differs"
}

# hash one fixed runtime-state tree without following links
runtime_state_identity() {
  local state_root=$1
  local entry relative kind mode uid gid size
  [[ ! -L "$state_root" ]] || fail "runtime state root is linked: $state_root"
  # represent one absent state root explicitly
  if [[ ! -e "$state_root" ]]; then
    printf '%s\n' absent
    return
  fi
  [[ -d "$state_root" ]] || fail "runtime state root is not a directory: $state_root"
  [[ -z "$(find -P "$state_root" -mindepth 1 ! -type d ! -type f -print -quit)" ]] ||
    fail "runtime state contains a non-file entry: $state_root"
  (
    # bind safe directory and regular-file identities
    while IFS= read -r -d '' entry; do
      relative=${entry#"$state_root"/}
      kind=$(stat -c '%F' "$entry")
      mode=$(stat -c '%a' "$entry")
      uid=$(stat -c '%u' "$entry")
      gid=$(stat -c '%g' "$entry")
      size=$(stat -c '%s' "$entry")
      printf '%s\0%s\0%s\0%s\0%s\0%s\0' \
        "$relative" "$kind" "$mode" "$uid" "$gid" "$size"
      # bind file content without loading it into memory
      if [[ -f "$entry" ]]; then
        sha256sum "$entry" | awk '{print $1}'
      fi
    done < <(find -P "$state_root" -mindepth 1 -print0 | LC_ALL=C sort -z)
  ) | sha256sum | awk '{print $1}'
}

# emit the two fixed affected runtime-state roots
runtime_state_roots() {
  local runtime_root=$1
  printf '%s\n' \
    "$runtime_root/xweather/adjustment-evidence" \
    "$runtime_root/xweather/adjustment-release-state"
}

# measure logical recovery bytes before allocating the backup
recovery_source_bytes() {
  local destination=$1
  local runtime_root=$2
  local total=0 value state_root
  # count the exact control files
  while read -r _ _ _ _ relative; do
    value=$(stat -c '%s' "$destination/$relative")
    total=$((total + value))
  done < <(predecessor_identity_manifest)
  # count only present fixed state roots
  while IFS= read -r state_root; do
    if [[ -e "$state_root" ]]; then
      value=$(du -sx --apparent-size --block-size=1 "$state_root" | awk '{print $1}')
      total=$((total + value))
    fi
  done < <(runtime_state_roots "$runtime_root")
  printf '%s\n' "$total"
}

# collect actual filesystem availability without allocation
maintenance_filesystem_sample() {
  local blocks block_size inodes
  read -r blocks block_size inodes < <(stat --file-system --format='%a %S %d' "$1")
  [[ "$blocks" =~ ^[0-9]+$ && "$block_size" =~ ^[1-9][0-9]*$ &&
    "$inodes" =~ ^[0-9]+$ ]] || fail "maintenance filesystem sample is invalid"
  printf '%s %s\n' "$((blocks * block_size))" "$inodes"
}

# reserve a measured transaction peak above unchanged capture floors
require_maintenance_capacity() {
  local root=$1 additional_bytes=$2 additional_inodes=$3
  local available_bytes available_inodes
  read -r available_bytes available_inodes < <(maintenance_filesystem_sample "$root")
  [[ "$available_bytes" =~ ^[0-9]+$ && "$available_inodes" =~ ^[0-9]+$ &&
    "$additional_bytes" =~ ^[0-9]+$ && "$additional_inodes" =~ ^[0-9]+$ ]] ||
    fail "maintenance capacity evidence is invalid"
  ((additional_bytes <= maintenance_backup_max_bytes + maintenance_backup_overhead_bytes &&
    additional_inodes <= 1000000)) || fail "maintenance transaction allocation exceeds its bound"
  ((available_bytes >= 2034155520 + additional_bytes &&
    available_inodes >= 32768 + additional_inodes)) ||
    fail "maintenance transaction would cross the protected byte or inode floor"
}

# bound copied blocks and inodes including directory and per-file metadata
recovery_source_allocation() {
  local destination=$1 runtime_root=$2 relative root entry size blocks
  local bytes=$maintenance_backup_overhead_bytes inodes=128
  # stream copied entries without retaining a lifetime array
  while IFS= read -r -d '' entry; do
    [[ ! -L "$entry" && ( -f "$entry" || -d "$entry" ) ]] ||
      fail "maintenance recovery entry is unsupported"
    read -r size blocks < <(stat -c '%s %b' "$entry")
    ((blocks * 512 <= size)) || size=$((blocks * 512))
    bytes=$((bytes + ((size + 4095) / 4096 + 1) * 4096))
    inodes=$((inodes + 1))
    ((bytes <= maintenance_backup_max_bytes && inodes <= 1000000)) ||
      fail "allocated recovery exceeds its byte or inode bound"
  done < <(
    # include every frozen predecessor file
    while read -r _ _ _ _ relative; do
      printf '%s\0' "$destination/$relative"
    done < <(predecessor_identity_manifest)
    # enumerate only the already-verified fixed state roots
    while IFS= read -r root; do
      if [[ -e "$root" ]]; then
        find -P "$root" -print0
      fi
    done < <(runtime_state_roots "$runtime_root")
  )
  printf '%s %s\n' "$bytes" "$inodes"
}

# copy every predecessor identity and affected state root
create_recovery_backup() {
  local destination=$1
  local runtime_root=$2
  local backup=$3
  local failure_step=$4
  local relative state_root index=0 identity_before identity_after
  mkdir -p "$backup/control" "$backup/state"
  # retain all forty-two exact predecessor files
  while read -r _ _ _ _ relative; do
    mkdir -p "$backup/control/$(dirname "$relative")"
    cp -a --reflink=never "$destination/$relative" "$backup/control/$relative"
  done < <(predecessor_identity_manifest)
  # retain the two fixed state trees with drift detection
  while IFS= read -r state_root; do
    identity_before=$(runtime_state_identity "$state_root")
    printf '%s\n' "$identity_before" >"$backup/state/$index.identity"
    if [[ "$identity_before" == absent ]]; then
      printf '%s\n' absent >"$backup/state/$index.absent"
    else
      cp -a --reflink=never "$state_root" "$backup/state/$index"
      identity_after=$(runtime_state_identity "$state_root")
      [[ "$identity_after" == "$identity_before" ]] ||
        fail "runtime state changed while being backed up: $state_root"
      [[ "$(runtime_state_identity "$backup/state/$index")" == "$identity_before" ]] ||
        fail "runtime state backup differs: $state_root"
    fi
    index=$((index + 1))
    # inject only bounded state-backup failures in tests
    if ((failure_step > 0 && failure_step == index)); then
      fail "internal state-backup failure probe $failure_step"
    fi
  done < <(runtime_state_roots "$runtime_root")
  [[ "$(du -sx --apparent-size --block-size=1 "$backup" | awk '{print $1}')" -le "$maintenance_backup_max_bytes" ]] ||
    fail "recovery backup exceeds four GiB"
}

# emit a binary identity manifest for every sealed backup member
backup_member_manifest() {
  local backup=$1
  local entry relative kind mode uid gid size sha
  # bind every directory and regular file except the two envelope files
  while IFS= read -r -d '' entry; do
    relative=${entry#"$backup"/}
    [[ "$relative" != members.manifest &&
      "$relative" != sealed-transaction.manifest ]] || continue
    if [[ -d "$entry" && ! -L "$entry" ]]; then
      kind='directory'
      sha=-
    elif [[ -f "$entry" && ! -L "$entry" ]]; then
      kind='file'
      sha=$(sha256sum "$entry" | awk '{print $1}')
    else
      fail "recovery backup contains an unsafe member: $relative"
    fi
    mode=$(stat -c '%a' "$entry")
    uid=$(stat -c '%u' "$entry")
    gid=$(stat -c '%g' "$entry")
    size=$(stat -c '%s' "$entry")
    printf '%s\0%s\0%s\0%s\0%s\0%s\0%s\0' \
      "$relative" "$kind" "$uid" "$gid" "$mode" "$size" "$sha"
  done < <(find -P "$backup" -mindepth 1 -print0 | LC_ALL=C sort -z)
}

# emit the exact sealed recovery envelope
expected_recovery_seal() {
  local backup=$1
  local candidate_digest
  candidate_digest=$(cat "$backup/v15-control.sha256")
  printf '%s\n' \
    'contractVersion=adjustment-maintenance-control-recovery/v2' \
    "transactionName=$(basename "$backup")" \
    "predecessorRelease=$maintenance_previous_release" \
    'recordedSourceControlVersion=13' \
    "recordedSourceControlSha256=$maintenance_recorded_control_plane_sha256" \
    'installedPredecessorControlVersion=14' \
    "predecessorControlSha256=$maintenance_installed_predecessor_control_plane_sha256" \
    "predecessorIdentityManifestSha256=$(predecessor_identity_manifest | sha256sum | awk '{print $1}')" \
    "candidateControlSha256=$candidate_digest" \
    "candidateIdentityManifestSha256=$(sha256sum "$backup/v15-identities.txt" | awk '{print $1}')" \
    "state0Identity=$(cat "$backup/state/0.identity")" \
    "state1Identity=$(cat "$backup/state/1.identity")" \
    "memberManifestSha256=$(sha256sum "$backup/members.manifest" | awk '{print $1}')"
}

# close one complete root-private recovery transaction
seal_recovery_backup() {
  local backup=$1
  local candidate_manifest=$2
  local candidate_digest=$3
  local scratch=$4
  local expected_uid=$5
  local expected_gid=$6
  local current_bytes manifest_bytes
  install -o "$expected_uid" -g "$expected_gid" -m 0600 \
    "$candidate_manifest" "$backup/v15-identities.txt"
  printf '%s\n' "$candidate_digest" >"$backup/v15-control.sha256"
  chmod 600 "$backup/v15-control.sha256"
  backup_member_manifest "$backup" >"$scratch/members.manifest"
  current_bytes=$(du -sx --apparent-size --block-size=1 "$backup" | awk '{print $1}')
  manifest_bytes=$(stat -c '%s' "$scratch/members.manifest")
  ((manifest_bytes <= maintenance_backup_overhead_bytes / 2 &&
    current_bytes + manifest_bytes + maintenance_backup_overhead_bytes / 2 <=
      maintenance_backup_max_bytes)) ||
    fail "sealed recovery envelope exceeds its bounded reserve"
  install -o "$expected_uid" -g "$expected_gid" -m 0600 \
    "$scratch/members.manifest" "$backup/members.manifest"
  expected_recovery_seal "$backup" >"$scratch/sealed-transaction.manifest"
  install -o "$expected_uid" -g "$expected_gid" -m 0600 \
    "$scratch/sealed-transaction.manifest" "$backup/sealed-transaction.manifest"
  [[ "$(du -sx --apparent-size --block-size=1 "$backup" | awk '{print $1}')" -le "$maintenance_backup_max_bytes" ]] ||
    fail "sealed recovery backup exceeds four GiB"
}

# require one private canonical backup root
verify_backup_root() {
  local backup_root=$1
  local expected_uid=$2
  local expected_gid=$3
  local mode
  require_owned_directory "$backup_root" "$expected_uid" "$expected_gid" \
    "control backup root"
  mode=$(stat -c '%a' "$backup_root")
  (((8#$mode & 077) == 0)) || fail "control backup root is not owner-private"
}

# require one regular fixed transaction lock file
prepare_transaction_lock() {
  local backup_root=$1
  local expected_uid=$2
  local expected_gid=$3
  local lock_path="$backup_root/.adjustment-maintenance-control-install.lock"
  # refuse a linked or foreign preexisting lock
  if [[ -e "$lock_path" || -L "$lock_path" ]]; then
    [[ -f "$lock_path" && ! -L "$lock_path" &&
      "$(stat -c '%u' "$lock_path")" == "$expected_uid" &&
      "$(stat -c '%g' "$lock_path")" == "$expected_gid" &&
      "$(stat -c '%a' "$lock_path")" == 600 ]] ||
      fail "maintenance control transaction lock is unsafe"
  else
    install -o "$expected_uid" -g "$expected_gid" -m 0600 /dev/null "$lock_path"
  fi
}

# require one direct sealed transaction from the fixed backup root
verify_sealed_recovery_backup() {
  local backup_root=$1
  local backup=$2
  local expected_uid=$3
  local expected_gid=$4
  local candidate_digest index=0 state_identity
  verify_backup_root "$backup_root" "$expected_uid" "$expected_gid"
  [[ "$(dirname "$backup")" == "$backup_root" &&
    "$(basename "$backup")" =~ ^v15-${maintenance_installed_predecessor_control_plane_sha256:0:12}\.[a-zA-Z0-9]{8}$ ]] ||
    fail "recovery backup is not a fixed-root transaction"
  require_owned_directory "$backup" "$expected_uid" "$expected_gid" \
    "recovery transaction"
  (((8#$(stat -c '%a' "$backup") & 077) == 0)) ||
    fail "recovery transaction is not owner-private"
  [[ "$(du -sx --apparent-size --block-size=1 "$backup" | awk '{print $1}')" -le "$maintenance_backup_max_bytes" ]] ||
    fail "recovery transaction exceeds four GiB"
  # require all closed envelope files
  for member in members.manifest sealed-transaction.manifest \
    v15-identities.txt v15-control.sha256 state/0.identity state/1.identity; do
    [[ -f "$backup/$member" && ! -L "$backup/$member" ]] ||
      fail "sealed recovery member is missing or linked: $member"
  done
  # require the two self-describing envelope files to remain private
  for member in members.manifest sealed-transaction.manifest; do
    [[ "$(stat -c '%u' "$backup/$member")" == "$expected_uid" &&
      "$(stat -c '%g' "$backup/$member")" == "$expected_gid" &&
      "$(stat -c '%a' "$backup/$member")" == 600 ]] ||
      fail "sealed recovery envelope owner or mode differs: $member"
  done
  candidate_digest=$(cat "$backup/v15-control.sha256")
  [[ "$candidate_digest" =~ ^[a-f0-9]{64}$ ]] ||
    fail "sealed candidate digest is invalid"
  verify_predecessor_identities "$backup/control" "$expected_uid" "$expected_gid"
  verify_candidate_identity_manifest_file "$backup/v15-identities.txt" \
    "$candidate_digest" "$expected_uid" "$expected_gid"
  # require both captured state identities and contents
  while ((index < 2)); do
    state_identity=$(cat "$backup/state/$index.identity")
    if [[ "$state_identity" == absent ]]; then
      [[ -f "$backup/state/$index.absent" && ! -e "$backup/state/$index" ]] ||
        fail "sealed absent state member differs: $index"
    else
      [[ "$(runtime_state_identity "$backup/state/$index")" == "$state_identity" ]] ||
        fail "sealed state identity differs: $index"
    fi
    index=$((index + 1))
  done
  cmp -s "$backup/members.manifest" <(backup_member_manifest "$backup") ||
    fail "sealed recovery member manifest differs"
  cmp -s "$backup/sealed-transaction.manifest" <(expected_recovery_seal "$backup") ||
    fail "sealed recovery transaction manifest differs"
}

# atomically replace one root-owned control file
install_atomic_control_file() (
  local source=$1
  local target=$2
  local mode=$3
  local uid=$4
  local gid=$5
  local temporary=''
  trap '[[ -z "$temporary" ]] || rm -f -- "$temporary"' EXIT
  temporary=$(mktemp "$(dirname "$target")/.adjustment-maintenance-control.XXXXXXXX")
  install -o "$uid" -g "$gid" -m "$mode" "$source" "$temporary"
  [[ "$(sha256sum "$temporary" | awk '{print $1}')" == "$(sha256sum "$source" | awk '{print $1}')" ]] ||
    fail "staged control file hash changed: $target"
  mv -Tf -- "$temporary" "$target"
)

# provide a no-op production injection hook
installation_failure_probe() {
  :
}

# require every live identity to be predecessor or sealed candidate
verify_recoverable_control_identities() {
  local destination=$1
  local candidate_manifest=$2
  local expected_uid=$3
  local expected_gid=$4
  local old_sha _uid _gid mode relative candidate_sha actual_sha actual_mode
  local -A candidate_hash=()
  verify_closed_control_paths "$destination"
  # index the sealed candidate hashes
  while read -r candidate_sha _ _ _ relative; do
    candidate_hash["$relative"]=$candidate_sha
  done <"$candidate_manifest"
  # allow only exact old or new members during recovery
  while read -r old_sha _uid _gid mode relative; do
    [[ -f "$destination/$relative" && ! -L "$destination/$relative" ]] ||
      fail "recoverable control path is missing or linked: $relative"
    actual_sha=$(sha256sum "$destination/$relative" | awk '{print $1}')
    printf -v actual_mode '%04o' "$((8#$(stat -c '%a' "$destination/$relative")))"
    [[ "$(stat -c '%u' "$destination/$relative")" == "$expected_uid" &&
      "$(stat -c '%g' "$destination/$relative")" == "$expected_gid" &&
      "$actual_mode" == "$mode" &&
      ("$actual_sha" == "$old_sha" || "$actual_sha" == "${candidate_hash[$relative]:-}") ]] ||
      fail "live control identity is outside the sealed transaction: $relative"
  done < <(predecessor_identity_manifest)
}

# restore control identities without overwriting live runtime state
restore_v15_transaction() (
  local destination=$1
  local runtime_root=$2
  local backup=$3
  local expected_uid=$4
  local expected_gid=$5
  local data_uid=$6
  local data_gid=$7
  local backup_root=$8
  local sha _uid _gid mode relative state_root index=0 expected_identity state_drift=0
  local -A backup_mode=()
  # keep nested assertions fatal even when the caller handles a failure
  fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
  }
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid" || exit 1
  verify_sealed_recovery_backup "$backup_root" "$backup" \
    "$expected_uid" "$expected_gid" || exit 1
  verify_recoverable_control_identities "$destination" \
    "$backup/v15-identities.txt" "$expected_uid" "$expected_gid" || exit 1
  # cache fixed predecessor modes
  while read -r sha _uid _gid mode relative; do
    backup_mode["$relative"]=$mode
  done < <(predecessor_identity_manifest)
  # restore every path except update first
  while read -r _ _ _ _ relative; do
    [[ "$relative" == deploy/scripts/update.sh ]] && continue
    verify_control_target_ancestry "$destination" "$relative" \
      "$expected_uid" "$expected_gid" || exit 1
    install_atomic_control_file "$backup/control/$relative" \
      "$destination/$relative" "${backup_mode["$relative"]}" \
      "$expected_uid" "$expected_gid" || exit 1
  done < <(predecessor_identity_manifest)
  verify_control_target_ancestry "$destination" deploy/scripts/update.sh \
    "$expected_uid" "$expected_gid" || exit 1
  install_atomic_control_file "$backup/control/deploy/scripts/update.sh" \
    "$destination/deploy/scripts/update.sh" \
    "${backup_mode["deploy/scripts/update.sh"]}" "$expected_uid" "$expected_gid" || exit 1
  # snapshots are evidence, not authority to delete later response-finish captures
  while IFS= read -r state_root; do
    expected_identity=$(cat "$backup/state/$index.identity")
    verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
      "$expected_gid" "$data_uid" "$data_gid" || exit 1
    # preserve all state because the inert installer never mutates it
    if [[ "$(runtime_state_identity "$state_root")" != "$expected_identity" ]]; then
      printf 'error: live runtime state advanced; preserved without rollback: %s\n' \
        "$state_root" >&2
      state_drift=2
    fi
    index=$((index + 1))
  done < <(runtime_state_roots "$runtime_root")
  verify_predecessor_identities "$destination" "$expected_uid" "$expected_gid" || exit 1
  exit "$state_drift"
)

# refuse control recovery that would orphan an independent dashboard
require_no_dashboard_override() {
  local destination=$1 marker
  # any committed or interrupted web state requires its own rollback first
  for marker in current-web-release previous-web-release active-web.env dashboard-web-transaction.env; do
    [[ ! -e "$destination/deploy/state/$marker" && ! -L "$destination/deploy/state/$marker" ]] ||
      fail "dashboard rollback is required before control recovery"
  done
}

# recover one interrupted v15 transaction
recover_v15_control_plane() {
  local destination=$1
  local backup_root=$2
  local runtime_root=$3
  local backup=$4
  local expected_uid=${5:-0}
  local expected_gid=${6:-0}
  local data_uid=${7:-10002}
  local data_gid=${8:-10002}
  local lock_fd
  [[ "$destination" == /* && "$backup_root" == /* && "$runtime_root" == /* &&
    "$backup" == /* ]] ||
    fail "recovery paths must be absolute"
  [[ "$(dirname "$backup_root")" == "$runtime_root" ]] ||
    fail "backup root is not the fixed runtime child"
  require_no_dashboard_override "$destination"
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid"
  verify_sealed_recovery_backup "$backup_root" "$backup" \
    "$expected_uid" "$expected_gid"
  require_maintenance_predecessor_release "$destination"
  prepare_transaction_lock "$backup_root" "$expected_uid" "$expected_gid"
  exec {lock_fd}>"$backup_root/.adjustment-maintenance-control-install.lock"
  flock -n "$lock_fd" || fail "another maintenance control transaction is in flight"
  # rerun every authority check after locking
  require_quiet_weather
  require_no_dashboard_override "$destination"
  require_maintenance_predecessor_release "$destination"
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid"
  verify_sealed_recovery_backup "$backup_root" "$backup" \
    "$expected_uid" "$expected_gid"
  verify_recoverable_control_identities "$destination" \
    "$backup/v15-identities.txt" "$expected_uid" "$expected_gid"
  restore_v15_transaction "$destination" "$runtime_root" "$backup" \
    "$expected_uid" "$expected_gid" "$data_uid" "$data_gid" "$backup_root"
  printf 'Recovered maintenance predecessor control plane: %s\n' \
    "$maintenance_installed_predecessor_control_plane_sha256"
}

# install one exact v15 candidate transaction
install_v15_control_plane() (
  local destination=$1
  local backup_root=$2
  local runtime_root=$3
  local archive=$4
  local archive_sha256=$5
  local candidate_digest=$6
  local failure_step=${7:-0}
  local expected_uid=${8:-0}
  local expected_gid=${9:-0}
  local data_uid=${10:-10002}
  local data_gid=${11:-10002}
  local stage='' backup='' status recovery_status lock_fd relative index=0 replacement_step=2
  local transaction_started=0
  local candidate current_manifest state_root expected_state
  local source_bytes recovery_bytes recovery_inodes scratch_bytes
  local -A predecessor_mode=()

  # restore control files and preserve live evidence after a post-backup failure
  # shellcheck disable=SC2329
  restore_on_exit() {
    status=$?
    trap - EXIT
    if ((status != 0 && transaction_started == 1)); then
      printf 'Restoring forty-two control identities while preserving live runtime state from %s\n' \
        "$backup" >&2
      # isolate recovery from conditional errexit suppression before capturing status
      set +e
      (
        set -e
        restore_v15_transaction "$destination" "$runtime_root" "$backup" \
          "$expected_uid" "$expected_gid" "$data_uid" "$data_gid" "$backup_root"
      )
      recovery_status=$?
      set -e
      ((recovery_status == 0)) || status=2
    elif ((status != 0)) && [[ -n "$backup" ]]; then
      rm -rf -- "$backup"
    fi
    [[ -z "$stage" ]] || rm -rf -- "$stage"
    exit "$status"
  }

  [[ "$destination" == /* && "$backup_root" == /* && "$runtime_root" == /* &&
    "$archive" == /* ]] || fail "handoff paths must be absolute"
  [[ "$(realpath -m "$destination")" == "$destination" && -d "$destination" &&
    ! -L "$destination" ]] || fail "deployment root is not a canonical directory"
  [[ "$(realpath -m "$backup_root")" == "$backup_root" &&
    "$backup_root" != "$destination"/* &&
    "$backup_root" != "$runtime_root/xweather/adjustment-evidence" &&
    "$backup_root" != "$runtime_root/xweather/adjustment-evidence"/* &&
    "$backup_root" != "$runtime_root/xweather/adjustment-release-state" &&
    "$backup_root" != "$runtime_root/xweather/adjustment-release-state"/* ]] ||
    fail "backup root must be canonical and outside deployment and affected state"
  [[ "$(realpath -m "$runtime_root")" == "$runtime_root" && -d "$runtime_root" &&
    ! -L "$runtime_root" ]] || fail "runtime root is not a canonical directory"
  [[ -f "$archive" && ! -L "$archive" ]] ||
    fail "candidate archive is missing or linked"
  [[ "$archive_sha256" =~ ^[a-f0-9]{64}$ &&
    "$candidate_digest" =~ ^[a-f0-9]{64}$ ]] ||
    fail "candidate hashes must be complete SHA-256 values"
  [[ "$failure_step" =~ ^(0|[1-9]|1[0-2])$ ]] ||
    fail "invalid internal failure probe"
  [[ "$(sha256sum "$archive" | awk '{print $1}')" == "$archive_sha256" ]] ||
    fail "candidate archive hash differs from the committed artifact"
  require_maintenance_predecessor_release "$destination"
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid"
  verify_predecessor_identities "$destination" "$expected_uid" "$expected_gid"

  # admit public staging on the same measured filesystem before allocating it
  [[ "$(stat -c '%d' "${TMPDIR:-/tmp}")" == "$(stat -c '%d' "$runtime_root")" ]] ||
    fail "maintenance staging filesystem differs from the protected runtime filesystem"
  scratch_bytes=$((3 * $(stat -c '%s' "$archive") + maintenance_backup_overhead_bytes))
  require_maintenance_capacity "$runtime_root" "$scratch_bytes" 128

  # validate the complete candidate before privileged destination writes
  stage=$(mktemp -d "${TMPDIR:-/tmp}/adjustment-maintenance-control.XXXXXXXX")
  trap restore_on_exit EXIT
  install -m 0600 "$archive" "$stage/candidate.tar"
  [[ "$(sha256sum "$stage/candidate.tar" | awk '{print $1}')" == "$archive_sha256" ]] ||
    fail "private candidate archive hash differs from the committed artifact"
  extract_candidate_archive "$stage/candidate.tar" "$stage"
  candidate="$stage/candidate"
  verify_candidate_identities "$candidate" "$candidate_digest"
  candidate_identity_manifest "$candidate" "$expected_uid" "$expected_gid" \
    >"$stage/candidate-identities"

  # prove the recovery bucket can contain this transaction
  source_bytes=$(recovery_source_bytes "$destination" "$runtime_root")
  ((source_bytes <= maintenance_backup_max_bytes - maintenance_backup_overhead_bytes)) ||
    fail "recovery source exceeds four GiB"

  read -r recovery_bytes recovery_inodes < <(recovery_source_allocation "$destination" "$runtime_root")
  require_maintenance_capacity "$runtime_root" "$recovery_bytes" "$recovery_inodes"

  # reread live authority immediately before the first privileged recovery write
  require_quiet_weather
  require_maintenance_predecessor_release "$destination"
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid"
  verify_predecessor_identities "$destination" "$expected_uid" "$expected_gid"
  [[ "$(dirname "$backup_root")" == "$runtime_root" ]] ||
    fail "backup root is not the fixed runtime child"
  # refuse a linked or foreign existing backup root before install touches it
  if [[ -e "$backup_root" || -L "$backup_root" ]]; then
    verify_backup_root "$backup_root" "$expected_uid" "$expected_gid"
  else
    install -d -o "$expected_uid" -g "$expected_gid" -m 0700 "$backup_root"
  fi
  verify_backup_root "$backup_root" "$expected_uid" "$expected_gid"
  prepare_transaction_lock "$backup_root" "$expected_uid" "$expected_gid"
  exec {lock_fd}>"$backup_root/.adjustment-maintenance-control-install.lock"
  flock -n "$lock_fd" || fail "another maintenance control installer is in flight"
  # rerun live authority after locking and before backup writes
  require_quiet_weather
  require_maintenance_predecessor_release "$destination"
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid"
  verify_predecessor_identities "$destination" "$expected_uid" "$expected_gid"
  # remeasure the sealed-copy reservation after the transaction lock is held
  require_maintenance_capacity "$runtime_root" "$recovery_bytes" "$recovery_inodes"
  backup=$(mktemp -d "$backup_root/v15-${maintenance_installed_predecessor_control_plane_sha256:0:12}.XXXXXXXX")
  chmod 700 "$backup"
  create_recovery_backup "$destination" "$runtime_root" "$backup" "$failure_step"
  seal_recovery_backup "$backup" "$stage/candidate-identities" \
    "$candidate_digest" "$stage" "$expected_uid" "$expected_gid"
  verify_sealed_recovery_backup "$backup_root" "$backup" \
    "$expected_uid" "$expected_gid"
  transaction_started=1
  # retain atomic replacement scratch after the complete backup has allocated
  require_maintenance_capacity "$runtime_root" "$maintenance_backup_overhead_bytes" 128

  # require state and predecessor stability immediately before replacement
  require_quiet_weather
  require_maintenance_predecessor_release "$destination"
  verify_fixed_ancestry "$destination" "$runtime_root" "$expected_uid" \
    "$expected_gid" "$data_uid" "$data_gid"
  verify_predecessor_identities "$destination" "$expected_uid" "$expected_gid"
  while IFS= read -r state_root; do
    expected_state=$(cat "$backup/state/$index.identity")
    [[ "$(runtime_state_identity "$state_root")" == "$expected_state" ]] ||
      fail "runtime state changed before control replacement: $state_root"
    index=$((index + 1))
  done < <(runtime_state_roots "$runtime_root")

  # cache the frozen safe destination modes
  while read -r _ _ _ mode relative; do
    predecessor_mode["$relative"]=$mode
  done < <(predecessor_identity_manifest)
  # install leaves, then ACL, common and update last
  for relative in "${maintenance_changed_paths[@]}"; do
    verify_control_target_ancestry "$destination" "$relative" \
      "$expected_uid" "$expected_gid"
    install_atomic_control_file "$candidate/$relative" "$destination/$relative" \
      "${predecessor_mode["$relative"]}" "$expected_uid" "$expected_gid"
    replacement_step=$((replacement_step + 1))
    # expose one bounded failure after every replacement
    if ((failure_step > 0 && failure_step == replacement_step)); then
      installation_failure_probe "$replacement_step" "$runtime_root"
      fail "internal control replacement failure probe $failure_step"
    fi
  done

  # verify the complete resulting identity and digest
  current_manifest="$stage/installed-identities"
  actual_identity_manifest "$destination" >"$current_manifest"
  cmp -s "$stage/candidate-identities" "$current_manifest" ||
    fail "installed v15 identity manifest differs from candidate"
  [[ "$(control_digest "$destination")" == "$candidate_digest" ]] ||
    fail "installed v15 control digest differs from candidate"
  index=0
  while IFS= read -r state_root; do
    expected_state=$(cat "$backup/state/$index.identity")
    [[ "$(runtime_state_identity "$state_root")" == "$expected_state" ]] ||
      fail "runtime state changed during control installation: $state_root"
    index=$((index + 1))
  done < <(runtime_state_roots "$runtime_root")
  transaction_started=0
  printf 'Installed inert maintenance v15 control plane: %s\nRetained recovery backup: %s\n' \
    "$candidate_digest" "$backup"
)

# replace only the two actual forced-command mirrors after verifying installed v15
install_v15_ssh_mirrors() (
  local destination=$1
  local backup_root=$2
  local dispatcher=$3
  local operations=$4
  local candidate_digest=$5
  local uid=$6
  local gid=$7
  local old_dispatcher=8f19f12b133fc96bbc289fc18c2fff3c76ad36fad32a2c7f7230368637353f2b
  local old_operations=41bd0cdc9bb0f73cfdc11805528af49ea22e5c9675303167fbe62a534b74a60c
  local new_dispatcher new_operations backup path sha mode
  local mutated=0 completed=0
  backup=
  [[ "$candidate_digest" =~ ^[a-f0-9]{64}$ &&
    "$(control_digest "$destination")" == "$candidate_digest" &&
    "$(awk -F= '$1 == "control_plane_version" {print $2}' "$destination/deploy/scripts/update.sh")" == 15 ]] ||
    fail "SSH mirrors require the exact installed v15 control identity"
  verify_backup_root "$backup_root" "$uid" "$gid"
  prepare_transaction_lock "$backup_root" "$uid" "$gid"
  exec {mirror_lock_fd}>"$backup_root/.adjustment-maintenance-control-install.lock"
  flock -n "$mirror_lock_fd" || fail "another maintenance control transaction is active"
  require_maintenance_capacity "$(dirname "$backup_root")" "$maintenance_backup_overhead_bytes" 8
  # refuse linked or foreign parent directories without repairing them
  for path in "$(dirname "$dispatcher")" "$(dirname "$operations")"; do
    require_owned_directory "$path" "$uid" "$gid" "SSH mirror parent"
    mode=$(stat -c '%a' "$path")
    (( (8#$mode & 0022) == 0 )) || fail "SSH mirror parent is externally writable"
  done
  # freeze both real predecessor executable identities before any backup mutation
  for path in "$dispatcher" "$operations"; do
    [[ -f "$path" && ! -L "$path" &&
      "$(realpath -e "$path")" == "$path" &&
      "$(stat -c '%u:%g:%a:%h' "$path")" == "$uid:$gid:755:1" ]] ||
      fail "SSH mirror identity is unsafe: $path"
  done
  new_dispatcher=$(sha256sum "$destination/deploy/scripts/ssh-dispatch.sh" | awk '{print $1}')
  new_operations=$(sha256sum "$destination/deploy/scripts/remote-ops.sh" | awk '{print $1}')
  # a fully completed exact pair is an idempotent retry
  if [[ "$(sha256sum "$dispatcher" | awk '{print $1}')" == "$new_dispatcher" &&
    "$(sha256sum "$operations" | awk '{print $1}')" == "$new_operations" ]]; then
    printf 'SSH mirrors already match exact installed v15 controls\n'
    return
  fi
  [[ "$(sha256sum "$dispatcher" | awk '{print $1}')" == "$old_dispatcher" &&
    "$(sha256sum "$operations" | awk '{print $1}')" == "$old_operations" ]] ||
    fail "SSH mirror predecessor differs"
  backup=$(mktemp -d "$backup_root/ssh-mirrors-v15.XXXXXXXX")
  chmod 700 "$backup"
  install -m 0600 "$dispatcher" "$backup/dispatcher"
  install -m 0600 "$operations" "$backup/operations"
  printf '%s dispatcher\n%s operations\n' "$old_dispatcher" "$old_operations" >"$backup/identities"
  chmod 600 "$backup/identities"
  sync -f "$backup/dispatcher"
  sync -f "$backup/operations"
  sync -f "$backup/identities"
  sync -f "$backup"
  # restore only verified original bytes when a pair replacement fails
  # shellcheck disable=SC2329
  restore_failed_mirrors() {
    local status=$?
    trap - EXIT
    if [[ "$mutated" == 1 && "$completed" == 0 ]]; then
      # refuse external drift rather than overwrite an unrelated privileged executable
      for path in "$dispatcher" "$operations"; do
        [[ -f "$path" && ! -L "$path" &&
          "$(stat -c '%u:%g:%a:%h' "$path")" == "$uid:$gid:755:1" ]] || exit 1
      done
      sha=$(sha256sum "$dispatcher" | awk '{print $1}')
      [[ "$sha" == "$old_dispatcher" || "$sha" == "$new_dispatcher" ]] || exit 1
      sha=$(sha256sum "$operations" | awk '{print $1}')
      [[ "$sha" == "$old_operations" || "$sha" == "$new_operations" ]] || exit 1
      [[ "$(sha256sum "$backup/dispatcher" | awk '{print $1}')" == "$old_dispatcher" &&
        "$(sha256sum "$backup/operations" | awk '{print $1}')" == "$old_operations" ]] || exit 1
      install_atomic_control_file "$backup/dispatcher" "$dispatcher" 0755 "$uid" "$gid" || exit 1
      install_atomic_control_file "$backup/operations" "$operations" 0755 "$uid" "$gid" || exit 1
    fi
    exit "$status"
  }
  trap restore_failed_mirrors EXIT
  mutated=1
  install_atomic_control_file "$destination/deploy/scripts/ssh-dispatch.sh" "$dispatcher" 0755 "$uid" "$gid"
  install_atomic_control_file "$destination/deploy/scripts/remote-ops.sh" "$operations" 0755 "$uid" "$gid"
  [[ "$(sha256sum "$dispatcher" | awk '{print $1}')" == "$new_dispatcher" &&
    "$(sha256sum "$operations" | awk '{print $1}')" == "$new_operations" ]] ||
    fail "installed SSH mirror pair differs"
  completed=1
  trap - EXIT
  printf 'SSH mirrors match exact installed v15 controls\nRetained SSH mirror backup: %s\n' "$backup"
)

# accept only one fixed production installation or recovery
main() {
  ((EUID == 0)) || fail "host administrator authority is required"
  # install only the actual fixed forced-command mirrors as a separate bounded transaction
  if (($# == 2)) && [[ "$1" == --install-ssh-mirrors ]]; then
    local ancestor mode
    for ancestor in /usr /usr/local /usr/local/bin /usr/local/sbin; do
      require_owned_directory "$ancestor" 0 0 "SSH mirror ancestry"
      mode=$(stat -c '%a' "$ancestor")
      (( (8#$mode & 0022) == 0 )) || fail "SSH mirror ancestor is externally writable"
    done
    install_v15_ssh_mirrors /opt/weather/current /var/lib/weather/control-plane-backups \
      /usr/local/bin/weather-ssh-dispatch /usr/local/sbin/weather-remote-ops "$2" 0 0
    return
  fi
  # recover one explicitly named retained transaction
  if (($# == 2)) && [[ "$1" == --recover ]]; then
    recover_v15_control_plane /opt/weather/current \
      /var/lib/weather/control-plane-backups /var/lib/weather "$2" \
      0 0 10002 10002
    return
  fi
  (($# == 3)) ||
    fail "usage: install-adjustment-recurring-control-plane.sh ARCHIVE ARCHIVE_SHA256 CANDIDATE_SHA256 | --recover BACKUP | --install-ssh-mirrors CANDIDATE_SHA256"
  install_v15_control_plane /opt/weather/current \
    /var/lib/weather/control-plane-backups /var/lib/weather \
    "$1" "$2" "$3" 0 0 0 10002 10002
}

# run only when invoked directly
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
