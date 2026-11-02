#!/bin/bash
set -euo pipefail

readonly successor_library_sha256=f3df96877b938b67ab0c8eac497b2ee07c9b78db77e25896002ef51c9928fcd9
readonly successor_source_release=2026.10.09-2
readonly successor_previous_release=2026.10.09-1
readonly successor_future_release=2026.10.09-3
readonly successor_source_control_version=15
readonly successor_control_version=16
readonly successor_source_control_sha256=af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52
readonly successor_recurring_previous_release=2026.10.09-1
readonly successor_recurring_previous_control_sha256=603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba
readonly successor_source_env_sha256=4d34c1d82ce5b02a0911cf0371ddb9c9be56c2bf5d9096f84a590d75c4b4f724
readonly successor_target_env_sha256=97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7
readonly successor_bridge_sha256=c944a152f3129d7bf3bf3b2dcd2b4d230ca58c64719360e22351eb8043779c62
readonly successor_settings_sha256=9d3911129378ef3b4e2c0a310d04900d0aefaeb7394a215314e9665518eb1275
readonly successor_task_recovery_max_bytes=$((16 * 1024 * 1024))
readonly successor_temporary_max_bytes=$((128 * 1024 * 1024))
readonly successor_envelope_reserve_bytes=$((1024 * 1024))
readonly successor_predecessor_manifest_sha256=d1f64bf6c03fa35d198d6e9edc58b9d9ac4225e829e2c41e68761fe5ca827536
readonly -a successor_changed_paths=(
  deploy/scripts/adjustment-evaluation-package.mjs
  deploy/scripts/common.sh
  deploy/scripts/update.sh
)
readonly -a successor_existing_task_backups=(
  v15-08b25ce2c80b.W53hmfJ4
  v15-08b25ce2c80b.Ips8xyrd
)

# load the exact frozen low-level installer library
successor_load_library() {
  local self_root library metadata
  self_root=$(realpath -e "$(dirname "${BASH_SOURCE[0]}")")
  library="$self_root/install-adjustment-recurring-control-plane.sh"
  # reject a substituted helper before sourcing code
  [[ -f "$library" && ! -L "$library" && "$(realpath -e "$library")" == "$library" ]] ||
    { printf 'error: successor installer library is missing or linked\n' >&2; return 1; }
  metadata=$(stat -c '%u:%g:%a:%h' "$library")
  [[ "$metadata" == "$EUID:$(id -g):700:1" &&
    "$(sha256sum "$library" | awk '{print $1}')" == "$successor_library_sha256" ]] ||
    { printf 'error: successor installer library identity differs\n' >&2; return 1; }
  # shellcheck source=install-adjustment-recurring-control-plane.sh disable=SC1091
  source "$library"
}
successor_load_library

# emit the exact committed AF80 predecessor identities
predecessor_identity_manifest() {
  cat <<'EOF'
0fa4b2b990345a110ab324e13bf2e3528c19d530bf9a4d2b8576d3b42eebeade 0 0 0644 deploy/compose.yaml
c598784b280e13b9f12982e93df751b1b9a4290bb3c7a5c1994808cb75e16223 0 0 0775 deploy/postgres/010-create-runtime-roles.sh
20f90bfd9f86f9c35a8c0fa540760c71a96e0a9d1d2a71049ca0fbcf15a3031f 0 0 0775 deploy/postgres/postgres-admin-entrypoint.sh
b1e1f92d24a84317797bbe1ead5b877d3335e1ccadc54cb2680a55d033a24cc5 0 0 0664 deploy/postgres/runtime-acl-v1.sql
94a91746fbe0018873d8f0ab3971619737d6a40f94c2c48fed6164570f915321 0 0 0644 deploy/postgres/runtime-acl-v2.sql
09a456e821cf39d03a257f4795a05fd9787e10893869c455df7ca78bd2d7919a 0 0 0755 deploy/scripts/adjustment-evaluation-export.sh
2eeeff30f0db00a366806e5c0109d4797b3ec9dcbe3340eb0c523f134e776dcc 0 0 0644 deploy/scripts/adjustment-evaluation-package.mjs
bec5e85e4ac4e78c04a0b6f85209c5ba0b0818cd5a37b27183de1d72c66c8453 0 0 0644 deploy/scripts/adjustment-evidence-store.mjs
3446f5d8a3840bf4caf57ee7c09bdfccaa546d69abb6f31888d6a51192273038 0 0 0775 deploy/scripts/backup-stream.sh
dc55c72f239d6e86bbbaa119e2e785bd090a86bab18b342524c9f453aaeffc15 0 0 0775 deploy/scripts/backup.sh
488d775b49604de72795e9b91f1f2796a15fb1f6569ef837a118fdc8a34f3c38 0 0 0755 deploy/scripts/common.sh
8b7b4b1fa44ad2969066487ced0269b9d594cba4b3c29d17d6bffb474e16a6e3 0 0 0664 deploy/scripts/compatibility-provider.mjs
50c70b59285d2dd10e5e63a5c04b7331cfadd1da7e3db872d8c5561839e54975 0 0 0644 deploy/scripts/forecast-adjustment-scorecard-contract.mjs
48db6635659da7dba0062594f9ae1146176c40d22a714a58b7675b3f1570b5f8 0 0 0775 deploy/scripts/forecast-training-export.sh
f1bd65685da06f0d6969ad9b96722c5edb6f030e291a375bfb3e36192db76787 0 0 0755 deploy/scripts/forecast-training-package.mjs
3dc0cf6b07f4a844f95fffd58352523636d7b93c4b51f42ad47e1161c7dd0266 0 0 0644 deploy/scripts/home-network.mjs
6cc70ddf1ad123c0ba4863405f0fe7b83b124ad5ff69fd126a295f354b6a4226 0 0 0755 deploy/scripts/install-adjustment-scorecard.sh
4b56fc50489d90c64271d129fb0bda63968eacfd54201e8ae7985c0df5d6227d 0 0 0664 deploy/scripts/migrate.mjs
e8e70038bf77a85ca5eafc8047800ebbd931edd81d5f58a0228908ea4858987e 0 0 0775 deploy/scripts/preflight-capacity.sh
d7f790b0cba619210777f1c797c8f8a569fc405af24b75077d47e6b50dc9ce05 0 0 0775 deploy/scripts/public-stations-backfill.sh
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
7991a5a4f99e0fc3bc60440f963ed17247eb3ca2a4d22fad07aec915e3c91c1f 0 0 0775 deploy/scripts/tempest-backfill.sh
56344b9e042cb12f56dd6af5c0a24cfea5c770ddc96d5a37517888e09da462f3 0 0 0775 deploy/scripts/tide-backfill.sh
1357ccaa97836106fff16b25621c8e0a73a65629db2193c7a90781d887d1858c 0 0 0755 deploy/scripts/update.sh
aadfcdfa9fbdf76761616675e8c52995840b5bf6489185e4ba8a8eb9589e4dad 0 0 0775 deploy/scripts/verify-static.sh
27ac1c81f2d178d1d36e5b350f294ce901873e35876ad5e128422141d7eab0cd 0 0 0644 deploy/scripts/weather-admin-store.mjs
0afc5d86542479d3dc214780035c509f15188cf323497c886ca6edbdd52167c5 0 0 0644 deploy/scripts/web-server.mjs
2a0006cf85e7b6416c4628bcef23ddd55553402f63382358b283e2f8c37ffa82 0 0 0664 deploy/scripts/xweather-tile-cache.mjs
2a2f3e0cc8f59369a5235454a4237eed91c60cdbe374dcd1c8c7821731e2e3d0 0 0 0664 deploy/scripts/xweather-usage-budget.mjs
4f8b4d58b8645f40e5da779857669114a2bec11cb720aedd7be340c3c6c1ac00 0 0 0664 deploy/sudoers/weather-ops
85d77f2e61be5dab2d387e867a8dfb4ff70a90e659c5e9e91d633858d0c3d4aa 0 0 0664 deploy/systemd/weather-backup-local.service
9fb0866ceb3b81f90e0c579628a8028bc807d3f7c1a7ac9ff3e37c505c2d02dd 0 0 0664 deploy/systemd/weather-backup-local.timer
12e33019380f77215a44340e4d6feb4a0dc13f76c968ab36c4501d039ec24de8 0 0 0664 deploy/systemd/weather-compose.service
EOF
}

# emit the exact three successor changes in install order
successor_changed_path_manifest() {
  printf '%s\n' "${successor_changed_paths[@]}"
}

# classify one exact successor change
successor_changed_path() {
  local requested=$1 relative
  # match only the reviewed paths
  for relative in "${successor_changed_paths[@]}"; do
    [[ "$requested" != "$relative" ]] || return 0
  done
  return 1
}

# require one private regular evidence file
successor_require_file_identity() {
  local path=$1 sha256=$2 uid=$3 gid=$4 mode=$5 label=$6
  [[ -f "$path" && ! -L "$path" && "$(stat -c '%u:%g:%a:%h' "$path")" == "$uid:$gid:$mode:1" &&
    "$(sha256sum "$path" | awk '{print $1}')" == "$sha256" ]] || fail "$label identity differs"
}

# require the exact source environment identity
successor_verify_source_environment_identity() {
  successor_require_file_identity "$1" "$successor_source_env_sha256" "$2" "$3" 600 \
    "source release environment"
}

# require the exact committed bridge environment identity
successor_verify_target_environment_identity() {
  successor_require_file_identity "$1" "$successor_target_env_sha256" "$2" "$3" 600 \
    "committed bridge environment"
}

# require the exact immutable settings authority
successor_verify_settings_identity() {
  successor_require_file_identity "$1" "$successor_settings_sha256" "$2" "$3" 600 \
    "forecast adjustment settings"
}

# require one safe state marker
successor_require_state_marker() {
  local path=$1 expected=$2 uid=$3 gid=$4 label=$5
  [[ -f "$path" && ! -L "$path" && "$(stat -c '%u:%g:%a:%h' "$path")" == "$uid:$gid:600:1" &&
    "$(cat "$path")" == "$expected" ]] || fail "$label differs"
}

# reject every future or in-flight release artifact
successor_require_no_active_journal() {
  local destination=$1 state="$1/deploy/state" releases="$1/deploy/releases" path
  # reject fixed release journals and bridge publication debris
  for path in \
    "$state/.v14-compatibility-bridge-v1.pending" \
    "$state/normal-activation-v1.env" \
    "$state/dashboard-web-transaction.env" \
    "$state/current-web-release" \
    "$state/previous-web-release" \
    "$state/active-web.env"; do
    [[ ! -e "$path" && ! -L "$path" ]] || fail "active release journal or override exists: $path"
  done
  # reject exact future release artifacts, including partials
  [[ -z "$(find -P "$releases" -maxdepth 1 -mindepth 1 \
    \( -name "${successor_future_release}*" -o -name ".${successor_future_release}.*" \) -print -quit)" ]] ||
    fail "future release artifact exists"
}

# require the exact committed bridge state
successor_require_source_state() {
  local destination=$1 runtime_root=$2 control_uid=$3 control_gid=$4 data_uid=$5 data_gid=$6
  local state="$destination/deploy/state" releases="$destination/deploy/releases"
  verify_fixed_ancestry "$destination" "$runtime_root" "$control_uid" "$control_gid" "$data_uid" "$data_gid"
  successor_require_state_marker "$state/current-release" "$successor_source_release" \
    "$control_uid" "$control_gid" "current release"
  successor_require_state_marker "$state/schema-release" "$successor_source_release" \
    "$control_uid" "$control_gid" "schema release"
  successor_require_state_marker "$state/previous-release" "$successor_previous_release" \
    "$control_uid" "$control_gid" "previous release"
  [[ -L "$state/active.env" && "$(readlink "$state/active.env")" == \
    "../releases/$successor_source_release.env" ]] || fail "active bridge release differs"
  successor_verify_source_environment_identity "$releases/$successor_previous_release.env" "$control_uid" "$control_gid"
  successor_verify_target_environment_identity "$releases/$successor_source_release.env" "$control_uid" "$control_gid"
  successor_require_file_identity "$state/v14-compatibility-bridge-v1" "$successor_bridge_sha256" \
    "$control_uid" "$control_gid" 600 "committed bridge receipt"
  successor_verify_settings_identity "$runtime_root/xweather/forecast-adjustment-settings.json" "$data_uid" "$data_gid"
  successor_require_no_active_journal "$destination"
}

# capture every preserved state identity without file contents
successor_state_snapshot() {
  local destination=$1 runtime_root=$2 control_uid=$3 control_gid=$4 data_uid=$5 data_gid=$6
  local state="$destination/deploy/state" releases="$destination/deploy/releases" path
  successor_require_source_state "$destination" "$runtime_root" "$control_uid" "$control_gid" "$data_uid" "$data_gid"
  # bind fixed regular evidence metadata and bytes
  for path in \
    "$releases/$successor_previous_release.env" "$releases/$successor_source_release.env" \
    "$state/v14-compatibility-bridge-v1" "$state/current-release" "$state/schema-release" \
    "$state/previous-release" "$runtime_root/xweather/forecast-adjustment-settings.json"; do
    printf 'file\0%s\0%s\0%s\0' "$path" "$(stat -c '%d:%i:%u:%g:%a:%h:%s' "$path")" \
      "$(sha256sum "$path" | awk '{print $1}')"
  done
  printf 'link\0%s\0%s\0%s\0' "$state/active.env" \
    "$(stat -c '%d:%i:%u:%g:%a:%h:%s' "$state/active.env")" "$(readlink "$state/active.env")"
  # bind both evidence trees without restoring them
  while IFS= read -r path; do
    printf 'tree\0%s\0%s\0' "$path" "$(runtime_state_identity "$path")"
  done < <(runtime_state_roots "$runtime_root")
}

# hash the complete preserved state snapshot
successor_state_snapshot_sha256() {
  successor_state_snapshot "$@" | sha256sum | awk '{print $1}'
}

# acquire the exact release-state directory lock
successor_acquire_state_lock() {
  local state=$1 uid=$2 gid=$3
  local -n descriptor=$4
  require_owned_directory "$state" "$uid" "$gid" "release state lock root"
  exec {descriptor}<"$state"
  flock --exclusive --nonblock "$descriptor" || fail "another Weather release transaction is in flight"
  [[ "$(stat -Lc '%d:%i' "/proc/self/fd/$descriptor")" == "$(stat -c '%d:%i' "$state")" ]] ||
    fail "release state lock identity changed"
}

# require no overlapping privileged lifecycle process
successor_require_quiet_weather() {
  require_quiet_weather
}

# verify the exact AF80 predecessor control plane
successor_verify_predecessor_identities() {
  local root=$1 expected_uid=$2 expected_gid=$3
  local sha _uid _gid mode relative actual_mode
  verify_closed_control_paths "$root"
  # compare all forty-two predecessor identities
  while read -r sha _uid _gid mode relative; do
    printf -v actual_mode '%04o' "$((8#$(stat -c '%a' "$root/$relative")))"
    [[ -f "$root/$relative" && ! -L "$root/$relative" &&
      "$(sha256sum "$root/$relative" | awk '{print $1}')" == "$sha" &&
      "$(stat -c '%u:%g' "$root/$relative")" == "$expected_uid:$expected_gid" && "$actual_mode" == "$mode" ]] ||
      fail "AF80 predecessor identity differs: $relative"
  done < <(predecessor_identity_manifest)
  [[ "$(control_digest "$root")" == "$successor_source_control_sha256" ]] ||
    fail "AF80 predecessor control digest differs"
}

# verify one exact successor candidate
successor_verify_candidate() {
  local candidate=$1 candidate_digest=$2 old_sha _uid _gid mode relative new_sha count=0
  local -A changed=()
  verify_closed_control_paths "$candidate"
  # permit changes only at the three reviewed paths
  while read -r old_sha _uid _gid mode relative; do
    [[ -f "$candidate/$relative" && ! -L "$candidate/$relative" ]] || fail "candidate path is missing: $relative"
    new_sha=$(sha256sum "$candidate/$relative" | awk '{print $1}')
    if [[ "$new_sha" != "$old_sha" ]]; then
      successor_changed_path "$relative" || fail "candidate changes an unreviewed path: $relative"
      changed["$relative"]=1
      count=$((count + 1))
    fi
  done < <(predecessor_identity_manifest)
  [[ "$count" == 3 && "${#changed[@]}" == 3 ]] || fail "candidate must change exactly three reviewed paths"
  # require all three reviewed changes
  for relative in "${successor_changed_paths[@]}"; do
    [[ -n "${changed[$relative]:-}" ]] || fail "candidate leaves a required path unchanged: $relative"
  done
  grep -Fxq 'control_plane_version=16' "$candidate/deploy/scripts/update.sh" || fail "candidate control version is not sixteen"
  grep -Fxq "recurring_previous_release=$successor_recurring_previous_release" \
    "$candidate/deploy/scripts/update.sh" || fail "candidate recurring predecessor release differs"
  grep -Fxq "recurring_previous_control_plane_sha256=$successor_recurring_previous_control_sha256" \
    "$candidate/deploy/scripts/update.sh" || fail "candidate recurring predecessor digest differs"
  grep -Fxq "v16_handoff_source_release=$successor_source_release" \
    "$candidate/deploy/scripts/update.sh" || fail "candidate handoff source release differs"
  grep -Fxq "v16_handoff_source_control_version=$successor_source_control_version" \
    "$candidate/deploy/scripts/update.sh" || fail "candidate handoff source control version differs"
  grep -Fxq "v16_handoff_source_control_sha256=$successor_source_control_sha256" \
    "$candidate/deploy/scripts/update.sh" || fail "candidate handoff source control digest differs"
  [[ "$(control_digest "$candidate")" == "$candidate_digest" ]] || fail "candidate control digest differs"
}

# verify one sealed successor candidate identity manifest
successor_verify_candidate_manifest() {
  local manifest=$1 digest=$2
  local old_sha _ _ mode relative new_sha actual_uid actual_gid actual_mode count=0 lines=0
  local -A expected=() changed=()
  # index the exact predecessor set
  while read -r old_sha _ _ mode relative; do expected["$relative"]="$old_sha $mode"; done \
    < <(predecessor_identity_manifest)
  # verify every candidate identity
  while read -r new_sha actual_uid actual_gid actual_mode relative; do
    lines=$((lines + 1))
    [[ "$new_sha" =~ ^[a-f0-9]{64}$ && "$actual_uid" == 0 && "$actual_gid" == 0 &&
      -n "${expected[$relative]:-}" && "$actual_mode" == "${expected[$relative]#* }" ]] ||
      fail "candidate identity manifest differs: $relative"
    old_sha=${expected[$relative]%% *}; unset "expected[$relative]"
    if [[ "$new_sha" != "$old_sha" ]]; then
      successor_changed_path "$relative" || fail "candidate manifest changes an unreviewed path"
      changed["$relative"]=1; count=$((count + 1))
    fi
  done <"$manifest"
  [[ "$lines" == 42 && "${#expected[@]}" == 0 && "$count" == 3 && "${#changed[@]}" == 3 ]] ||
    fail "candidate identity manifest is incomplete"
  [[ "$(control_digest_from_identity_manifest "$manifest")" == "$digest" ]] ||
    fail "candidate identity manifest digest differs"
}

# emit canonical successor identities with policy owners
successor_candidate_identity_manifest() {
  local candidate=$1 _sha _uid _gid mode relative
  # retain the predecessor order and modes
  while read -r _sha _uid _gid mode relative; do
    printf '%s 0 0 %s %s\n' "$(sha256sum "$candidate/$relative" | awk '{print $1}')" \
      "$mode" "$relative"
  done < <(predecessor_identity_manifest)
}

# verify installed successor bytes against canonical identities
successor_verify_live_candidate_identities() {
  local root=$1 manifest=$2 uid=$3 gid=$4 sha _uid _gid mode relative actual_mode lines=0
  # verify every live owner, mode and content identity
  while read -r sha _uid _gid mode relative; do
    lines=$((lines + 1))
    printf -v actual_mode '%04o' "$((8#$(stat -c '%a' "$root/$relative")))"
    [[ "$_uid" == 0 && "$_gid" == 0 && -f "$root/$relative" && ! -L "$root/$relative" &&
      "$(sha256sum "$root/$relative" | awk '{print $1}')" == "$sha" &&
      "$(stat -c '%u:%g' "$root/$relative")" == "$uid:$gid" && "$actual_mode" == "$mode" ]] ||
      fail "installed successor identity differs: $relative"
  done <"$manifest"
  [[ "$lines" == 42 ]] || fail "installed successor identity manifest is incomplete"
}

# measure one file or tree allocation conservatively
successor_allocation_entry() {
  local path=$1 entry size blocks bytes=0 inodes=0
  # count every directory and regular file without links
  while IFS= read -r -d '' entry; do
    [[ ! -L "$entry" && ( -f "$entry" || -d "$entry" ) ]] || fail "recovery source entry is unsafe"
    read -r size blocks < <(stat -c '%s %b' "$entry")
    ((blocks * 512 <= size)) || size=$((blocks * 512))
    bytes=$((bytes + ((size + 4095) / 4096 + 1) * 4096)); inodes=$((inodes + 1))
  done < <(find -P "$path" -print0)
  printf '%s %s\n' "$bytes" "$inodes"
}

# derive the bounded new retained recovery allocation
successor_recovery_allocation() {
  local destination=$1 runtime_root=$2 bytes=$successor_envelope_reserve_bytes inodes=128
  local path value count
  # charge all controls and fixed evidence sources
  while read -r _ _ _ _ path; do
    read -r value count < <(successor_allocation_entry "$destination/$path")
    bytes=$((bytes + value)); inodes=$((inodes + count))
  done < <(predecessor_identity_manifest)
  for path in "$destination/deploy/releases/$successor_previous_release.env" \
    "$destination/deploy/releases/$successor_source_release.env" \
    "$destination/deploy/state/v14-compatibility-bridge-v1" \
    "$runtime_root/xweather/forecast-adjustment-settings.json"; do
    read -r value count < <(successor_allocation_entry "$path")
    bytes=$((bytes + value)); inodes=$((inodes + count))
  done
  while IFS= read -r path; do
    # charge only present evidence trees
    if [[ -e "$path" ]]; then
      read -r value count < <(successor_allocation_entry "$path")
      bytes=$((bytes + value)); inodes=$((inodes + count))
    fi
  done < <(runtime_state_roots "$runtime_root")
  printf '%s %s\n' "$bytes" "$inodes"
}

# measure the exact retained task-owned recovery set
successor_existing_task_recovery_bytes() {
  local backup_root=$1 uid=$2 gid=$3 name total=0 value
  # charge only the two frozen task backups
  for name in "${successor_existing_task_backups[@]}"; do
    require_owned_directory "$backup_root/$name" "$uid" "$gid" "retained task recovery $name"
    (( (8#$(stat -c '%a' "$backup_root/$name") & 077) == 0 )) ||
      fail "retained task recovery is not private: $name"
    value=$(du -sx --block-size=1 "$backup_root/$name" | awk '{print $1}')
    total=$((total + value))
  done
  printf '%s\n' "$total"
}

# enforce the exact sixteen MiB task ledger
successor_require_task_recovery_capacity() {
  local existing=$1 new_bytes=$2
  [[ "$existing" =~ ^[0-9]+$ && "$new_bytes" =~ ^[0-9]+$ ]] || fail "task recovery accounting is invalid"
  ((existing + new_bytes <= successor_task_recovery_max_bytes)) ||
    fail "task recovery allocation exceeds sixteen MiB"
}

# emit a closed successor backup member manifest
successor_backup_member_manifest() {
  local backup=$1 entry relative kind sha
  # bind every member except recursive envelope files
  while IFS= read -r -d '' entry; do
    relative=${entry#"$backup"/}
    [[ "$relative" != members.manifest && "$relative" != sealed-transaction.manifest ]] || continue
    if [[ -d "$entry" && ! -L "$entry" ]]; then
      kind=directory; sha=-
    elif [[ -f "$entry" && ! -L "$entry" ]]; then
      kind='file'; sha=$(sha256sum "$entry" | awk '{print $1}')
    else
      fail "successor recovery contains an unsafe member: $relative"
    fi
    printf '%s\0%s\0%s\0%s\0%s\0%s\0%s\0' "$relative" "$kind" \
      "$(stat -c '%u' "$entry")" "$(stat -c '%g' "$entry")" "$(stat -c '%a' "$entry")" \
      "$(stat -c '%s' "$entry")" "$sha"
  done < <(find -P "$backup" -mindepth 1 -print0 | LC_ALL=C sort -z)
}

# emit the exact successor recovery seal
successor_expected_seal() {
  local backup=$1
  printf '%s\n' \
    'contractVersion=adjustment-maintenance-control-successor-recovery/v1' \
    "transactionName=$(basename "$backup")" \
    "sourceRelease=$successor_source_release" \
    "sourceControlVersion=$successor_source_control_version" \
    "sourceControlSha256=$successor_source_control_sha256" \
    "successorControlVersion=$successor_control_version" \
    "successorControlSha256=$(cat "$backup/successor-control.sha256")" \
    "predecessorIdentityManifestSha256=$successor_predecessor_manifest_sha256" \
    "candidateIdentityManifestSha256=$(sha256sum "$backup/successor-identities.txt" | awk '{print $1}')" \
    "sourceStateSnapshotSha256=$(sha256sum "$backup/source-state.snapshot" | awk '{print $1}')" \
    "bridgeStateSha256=$successor_bridge_sha256" \
    "memberManifestSha256=$(sha256sum "$backup/members.manifest" | awk '{print $1}')"
}

# create one complete successor recovery backup
successor_create_backup() {
  local destination=$1 runtime_root=$2 backup=$3 candidate_manifest=$4 candidate_digest=$5
  local uid=$6 gid=$7 state_root index=0 relative
  mkdir -p "$backup/control" "$backup/state" "$backup/evidence"
  # copy every predecessor control
  while read -r _ _ _ _ relative; do
    mkdir -p "$backup/control/$(dirname "$relative")"
    cp -a --reflink=never "$destination/$relative" "$backup/control/$relative"
  done < <(predecessor_identity_manifest)
  # snapshot both live evidence trees without restore authority
  while IFS= read -r state_root; do
    printf '%s\n' "$(runtime_state_identity "$state_root")" >"$backup/state/$index.identity"
    if [[ -e "$state_root" ]]; then
      cp -a --reflink=never "$state_root" "$backup/state/$index"
    else
      printf '%s\n' absent >"$backup/state/$index.absent"
    fi
    index=$((index + 1))
  done < <(runtime_state_roots "$runtime_root")
  install -o "$uid" -g "$gid" -m 0600 "$destination/deploy/releases/$successor_previous_release.env" \
    "$backup/evidence/source-release.env"
  install -o "$uid" -g "$gid" -m 0600 "$destination/deploy/releases/$successor_source_release.env" \
    "$backup/evidence/bridge-release.env"
  install -o "$uid" -g "$gid" -m 0600 "$destination/deploy/state/v14-compatibility-bridge-v1" \
    "$backup/evidence/bridge-state"
  install -o "$uid" -g "$gid" -m 0600 "$runtime_root/xweather/forecast-adjustment-settings.json" \
    "$backup/evidence/forecast-adjustment-settings.json"
  install -o "$uid" -g "$gid" -m 0600 "$candidate_manifest" "$backup/successor-identities.txt"
  printf '%s\n' "$candidate_digest" >"$backup/successor-control.sha256"
  chmod 600 "$backup/successor-control.sha256"
}

# seal and sync one successor recovery backup
successor_seal_backup() {
  local backup=$1 state_snapshot=$2 scratch=$3 uid=$4 gid=$5 file
  install -o "$uid" -g "$gid" -m 0600 "$state_snapshot" "$backup/source-state.snapshot"
  successor_backup_member_manifest "$backup" >"$scratch/members.manifest"
  install -o "$uid" -g "$gid" -m 0600 "$scratch/members.manifest" "$backup/members.manifest"
  successor_expected_seal "$backup" >"$scratch/sealed-transaction.manifest"
  install -o "$uid" -g "$gid" -m 0600 "$scratch/sealed-transaction.manifest" "$backup/sealed-transaction.manifest"
  # durably close every seal operand before replacement
  while IFS= read -r -d '' file; do
    [[ -f "$file" ]] && sync -f "$file"
  done < <(find -P "$backup" -type f -print0)
  sync -f "$backup"; sync -f "$(dirname "$backup")"
}

# verify one complete successor recovery backup
successor_verify_backup() {
  local backup_root=$1 backup=$2 uid=$3 gid=$4 digest index=0 state_root member
  verify_backup_root "$backup_root" "$uid" "$gid"
  [[ "$(dirname "$backup")" == "$backup_root" &&
    "$(basename "$backup")" =~ ^successor-af80d5752590\.[a-zA-Z0-9]{8}$ ]] ||
    fail "successor recovery is outside its fixed root"
  require_owned_directory "$backup" "$uid" "$gid" "successor recovery transaction"
  # require the complete private envelope
  for member in members.manifest sealed-transaction.manifest successor-identities.txt \
    successor-control.sha256 source-state.snapshot; do
    [[ -f "$backup/$member" && ! -L "$backup/$member" &&
      "$(stat -c '%u:%g:%a:%h' "$backup/$member")" == "$uid:$gid:600:1" ]] ||
      fail "successor recovery envelope differs: $member"
  done
  digest=$(cat "$backup/successor-control.sha256")
  [[ "$digest" =~ ^[a-f0-9]{64}$ ]] || fail "successor recovery digest is invalid"
  successor_verify_predecessor_identities "$backup/control" "$uid" "$gid"
  successor_verify_candidate_manifest "$backup/successor-identities.txt" "$digest"
  cmp -s "$backup/members.manifest" <(successor_backup_member_manifest "$backup") ||
    fail "successor recovery member manifest differs"
  cmp -s "$backup/sealed-transaction.manifest" <(successor_expected_seal "$backup") ||
    fail "successor recovery seal differs"
  # verify both captured evidence trees
  while ((index < 2)); do
    state_root="$backup/state/$index"
    if [[ "$(cat "$state_root.identity")" == absent ]]; then
      [[ -f "$state_root.absent" && ! -e "$state_root" ]] || fail "absent evidence snapshot differs"
    else
      [[ "$(runtime_state_identity "$state_root")" == "$(cat "$state_root.identity")" ]] ||
        fail "evidence snapshot differs"
    fi
    index=$((index + 1))
  done
}

# install and sync one exact control file
successor_install_control_file() {
  install_atomic_control_file "$@"
  sync -f "$2"; sync -f "$(dirname "$2")"
}

# allow only old or sealed successor identities during recovery
successor_verify_recoverable_identities() {
  local destination=$1 manifest=$2 uid=$3 gid=$4 old_sha _ _ mode relative new_sha actual_sha actual_mode
  local -A successor=()
  # index the sealed successor set
  while read -r new_sha _ _ _ relative; do successor["$relative"]=$new_sha; done <"$manifest"
  # reject every identity outside the transaction
  while read -r old_sha _ _ mode relative; do
    actual_sha=$(sha256sum "$destination/$relative" | awk '{print $1}')
    printf -v actual_mode '%04o' "$((8#$(stat -c '%a' "$destination/$relative")))"
    [[ "$(stat -c '%u:%g' "$destination/$relative")" == "$uid:$gid" && "$actual_mode" == "$mode" &&
      ( "$actual_sha" == "$old_sha" || "$actual_sha" == "${successor[$relative]:-}" ) ]] ||
      fail "live control identity is outside the successor transaction: $relative"
  done < <(predecessor_identity_manifest)
}

# build canonical handoff receipt bytes
successor_handoff_json() {
  local candidate_digest=$1 candidate_manifest_sha=$2 installed_at=$3
  python3 -I -B - "$candidate_digest" "$candidate_manifest_sha" "$installed_at" \
    "$successor_predecessor_manifest_sha256" "$successor_bridge_sha256" <<'PYJSON'
import hashlib,json,sys
candidate,manifest,installed,predecessor,bridge=sys.argv[1:]
body={"bridgeStateSha256":bridge,"candidateIdentityManifestSha256":manifest,
"contractVersion":"weather-control-plane-handoff/v16-v1","installedAt":installed,
"predecessorIdentityManifestSha256":predecessor,"sourceControlSha256":"af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52",
"sourceControlVersion":15,"sourceRelease":"2026.10.09-2","successorControlSha256":candidate,
"successorControlVersion":16}
canonical=json.dumps(body,sort_keys=True,separators=(",",":"))+"\n"
body["receiptSha256"]=hashlib.sha256(canonical.encode()).hexdigest()
print(json.dumps(body,sort_keys=True,separators=(",",":")))
PYJSON
}

# validate one exact successor handoff receipt
successor_verify_handoff() {
  local receipt=$1 candidate_digest=$2 candidate_manifest_sha=$3 uid=$4 gid=$5
  [[ -f "$receipt" && ! -L "$receipt" && "$(stat -c '%u:%g:%a:%h' "$receipt")" == "$uid:$gid:600:1" ]] ||
    fail "successor handoff receipt metadata differs"
  python3 -I -B - "$receipt" "$candidate_digest" "$candidate_manifest_sha" \
    "$successor_predecessor_manifest_sha256" "$successor_bridge_sha256" <<'PYVERIFY'
import hashlib,json,sys
path,candidate,manifest,predecessor,bridge=sys.argv[1:]
raw=open(path,"rb").read(); assert raw.endswith(b"\n") and len(raw)<=4096
value=json.loads(raw); expected={"bridgeStateSha256","candidateIdentityManifestSha256","contractVersion",
"installedAt","predecessorIdentityManifestSha256","receiptSha256","sourceControlSha256",
"sourceControlVersion","sourceRelease","successorControlSha256","successorControlVersion"}
assert set(value)==expected and value["contractVersion"]=="weather-control-plane-handoff/v16-v1"
assert value["sourceRelease"]=="2026.10.09-2" and value["sourceControlVersion"]==15
assert value["sourceControlSha256"]=="af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52"
assert value["successorControlVersion"]==16 and value["successorControlSha256"]==candidate
assert value["candidateIdentityManifestSha256"]==manifest
assert value["predecessorIdentityManifestSha256"]==predecessor and value["bridgeStateSha256"]==bridge
receipt=value.pop("receiptSha256"); canonical=json.dumps(value,sort_keys=True,separators=(",",":"))+"\n"
assert receipt==hashlib.sha256(canonical.encode()).hexdigest()
PYVERIFY
}

# publish the handoff as the final durable mutation
successor_publish_handoff() {
  local receipt=$1 candidate_digest=$2 candidate_manifest_sha=$3 uid=$4 gid=$5 installed_at bytes
  [[ ! -e "$receipt" && ! -L "$receipt" ]] || fail "successor handoff receipt already exists"
  installed_at=$(date -u +%FT%TZ)
  bytes=$(successor_handoff_json "$candidate_digest" "$candidate_manifest_sha" "$installed_at")
  python3 -I -B - "$receipt" "$bytes" "$uid" "$gid" <<'PYPUBLISH'
import os,sys
path,raw,uid,gid=sys.argv[1],sys.argv[2].encode()+b"\n",int(sys.argv[3]),int(sys.argv[4])
fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
try:
 os.fchown(fd,uid,gid); os.write(fd,raw); os.fsync(fd)
finally: os.close(fd)
dfd=os.open(os.path.dirname(path),os.O_RDONLY|os.O_DIRECTORY)
try: os.fsync(dfd)
finally: os.close(dfd)
PYPUBLISH
  successor_verify_handoff "$receipt" "$candidate_digest" "$candidate_manifest_sha" "$uid" "$gid"
}

# provide one no-op production failure hook
successor_failure_probe() {
  :
}

# restore all AF80 controls while preserving evidence
successor_restore_transaction() {
  local destination=$1 runtime_root=$2 backup=$3 backup_root=$4 uid=$5 gid=$6 data_uid=$7 data_gid=$8
  local candidate_digest candidate_manifest_sha snapshot_now relative _ _ _ mode receipt recovery_state=0
  successor_verify_backup "$backup_root" "$backup" "$uid" "$gid"
  successor_verify_recoverable_identities "$destination" "$backup/successor-identities.txt" "$uid" "$gid"
  candidate_digest=$(cat "$backup/successor-control.sha256")
  candidate_manifest_sha=$(sha256sum "$backup/successor-identities.txt" | awk '{print $1}')
  receipt="$destination/deploy/state/control-plane-handoff-v16-v1.json"
  # restore every predecessor control without touching evidence
  while read -r _ _ _ mode relative; do
    successor_install_control_file "$backup/control/$relative" "$destination/$relative" "$mode" "$uid" "$gid"
  done < <(predecessor_identity_manifest)
  successor_verify_predecessor_identities "$destination" "$uid" "$gid"
  snapshot_now=$(successor_state_snapshot_sha256 "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid")
  if [[ "$snapshot_now" == "$(sha256sum "$backup/source-state.snapshot" | awk '{print $1}')" ]]; then
    # remove only the exact transaction receipt after unchanged-state rollback
    if [[ -e "$receipt" || -L "$receipt" ]]; then
      successor_verify_handoff "$receipt" "$candidate_digest" "$candidate_manifest_sha" "$uid" "$gid"
      rm -f -- "$receipt"
      sync -f "$(dirname "$receipt")"
    fi
  else
    printf 'error: preserved source state advanced during successor recovery\n' >&2
    recovery_state=2
  fi
  return "$recovery_state"
}

# recover one explicitly named successor transaction
recover_successor_control_plane_v1() {
  local destination=$1 backup_root=$2 runtime_root=$3 backup=$4 uid=${5:-0} gid=${6:-0}
  local data_uid=${7:-10002} data_gid=${8:-10002} state_fd backup_fd
  [[ "$destination" == /* && "$backup_root" == /* && "$runtime_root" == /* && "$backup" == /* ]] ||
    fail "successor recovery paths must be absolute"
  successor_acquire_state_lock "$destination/deploy/state" "$uid" "$gid" state_fd
  verify_backup_root "$backup_root" "$uid" "$gid"
  prepare_transaction_lock "$backup_root" "$uid" "$gid"
  exec {backup_fd}>"$backup_root/.adjustment-maintenance-control-install.lock"
  flock -n "$backup_fd" || fail "another maintenance control transaction is in flight"
  successor_require_quiet_weather
  successor_require_source_state "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid"
  successor_restore_transaction "$destination" "$runtime_root" "$backup" "$backup_root" \
    "$uid" "$gid" "$data_uid" "$data_gid"
  printf 'Recovered AF80 successor predecessor controls\n'
  : "$state_fd"
}

# install one exact AF80-to-v16 successor transaction
install_successor_control_plane_v1() (
  local destination=$1 backup_root=$2 runtime_root=$3 archive=$4 archive_sha=$5 candidate_digest=$6
  local failure_step=${7:-0} uid=${8:-0} gid=${9:-0} data_uid=${10:-10002} data_gid=${11:-10002}
  local stage='' backup='' status recovery_status state_fd backup_fd candidate candidate_manifest
  local existing_bytes recovery_bytes recovery_inodes scratch_bytes snapshot_sha receipt relative _ _ _ mode
  local transaction_started=0 replacement_step=0 candidate_manifest_sha
  # restore controls after any post-backup failure
  # shellcheck disable=SC2329
  restore_successor_on_exit() {
    status=$?
    trap - EXIT
    if ((status != 0 && transaction_started == 1)); then
      set +e
      ( set -e; successor_restore_transaction "$destination" "$runtime_root" "$backup" "$backup_root" \
        "$uid" "$gid" "$data_uid" "$data_gid" )
      recovery_status=$?
      set -e
      ((recovery_status == 0)) || status=2
    elif ((status != 0)) && [[ -n "$backup" ]]; then
      rm -rf -- "$backup"
    fi
    [[ -z "$stage" ]] || rm -rf -- "$stage"
    exit "$status"
  }
  [[ "$destination" == /* && "$backup_root" == /* && "$runtime_root" == /* && "$archive" == /* ]] ||
    fail "successor install paths must be absolute"
  [[ "$archive_sha" =~ ^[a-f0-9]{64}$ && "$candidate_digest" =~ ^[a-f0-9]{64}$ &&
    "$failure_step" =~ ^[0-5]$ ]] || fail "successor install arguments are invalid"
  [[ -f "$archive" && ! -L "$archive" && "$(sha256sum "$archive" | awk '{print $1}')" == "$archive_sha" ]] ||
    fail "successor candidate archive differs"
  receipt="$destination/deploy/state/control-plane-handoff-v16-v1.json"
  [[ ! -e "$receipt" && ! -L "$receipt" ]] || fail "successor handoff receipt already exists"
  successor_verify_predecessor_identities "$destination" "$uid" "$gid"
  successor_require_source_state "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid"
  # keep public staging on the measured runtime filesystem
  [[ "$(stat -c '%d' "${TMPDIR:-/tmp}")" == "$(stat -c '%d' "$runtime_root")" ]] ||
    fail "successor staging filesystem differs from the protected runtime filesystem"
  # charge temporary archive work separately from retained recovery
  scratch_bytes=$((3 * $(stat -c '%s' "$archive") + successor_envelope_reserve_bytes))
  ((scratch_bytes <= successor_temporary_max_bytes)) || fail "successor temporary allocation exceeds 128 MiB"
  require_maintenance_capacity "$runtime_root" "$scratch_bytes" 128
  stage=$(mktemp -d "${TMPDIR:-/tmp}/adjustment-control-successor.XXXXXXXX")
  trap restore_successor_on_exit EXIT
  install -m 0600 "$archive" "$stage/candidate.tar"
  extract_candidate_archive "$stage/candidate.tar" "$stage"
  candidate="$stage/candidate"
  successor_verify_candidate "$candidate" "$candidate_digest"
  candidate_manifest="$stage/candidate-identities"
  successor_candidate_identity_manifest "$candidate" >"$candidate_manifest"
  candidate_manifest_sha=$(sha256sum "$candidate_manifest" | awk '{print $1}')
  successor_verify_candidate_manifest "$candidate_manifest" "$candidate_digest"
  read -r recovery_bytes recovery_inodes < <(successor_recovery_allocation "$destination" "$runtime_root")
  # acquire the state lock before the backup transaction lock
  successor_acquire_state_lock "$destination/deploy/state" "$uid" "$gid" state_fd
  successor_require_quiet_weather
  successor_require_source_state "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid"
  successor_verify_predecessor_identities "$destination" "$uid" "$gid"
  verify_backup_root "$backup_root" "$uid" "$gid"
  prepare_transaction_lock "$backup_root" "$uid" "$gid"
  exec {backup_fd}>"$backup_root/.adjustment-maintenance-control-install.lock"
  flock -n "$backup_fd" || fail "another maintenance control transaction is in flight"
  # reread every authority under both locks
  successor_require_quiet_weather
  successor_require_source_state "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid"
  successor_verify_predecessor_identities "$destination" "$uid" "$gid"
  [[ -z "$(find -P "$backup_root" -maxdepth 1 -mindepth 1 -type d -name 'successor-af80d5752590.*' -print -quit)" ]] ||
    fail "an unresolved successor recovery transaction exists"
  existing_bytes=$(successor_existing_task_recovery_bytes "$backup_root" "$uid" "$gid")
  successor_require_task_recovery_capacity "$existing_bytes" "$recovery_bytes"
  require_maintenance_capacity "$runtime_root" "$recovery_bytes" "$recovery_inodes"
  snapshot_sha=$(successor_state_snapshot_sha256 "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid")
  successor_state_snapshot "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid" \
    >"$stage/source-state.snapshot"
  [[ "$(sha256sum "$stage/source-state.snapshot" | awk '{print $1}')" == "$snapshot_sha" ]] ||
    fail "source state changed during snapshot"
  backup=$(mktemp -d "$backup_root/successor-af80d5752590.XXXXXXXX")
  chmod 700 "$backup"
  successor_create_backup "$destination" "$runtime_root" "$backup" "$candidate_manifest" \
    "$candidate_digest" "$uid" "$gid"
  successor_seal_backup "$backup" "$stage/source-state.snapshot" "$stage" "$uid" "$gid"
  successor_verify_backup "$backup_root" "$backup" "$uid" "$gid"
  successor_require_task_recovery_capacity "$existing_bytes" \
    "$(du -sx --block-size=1 "$backup" | awk '{print $1}')"
  transaction_started=1
  # exercise rollback immediately after the durable backup
  if ((failure_step == 1)); then
    successor_failure_probe 1 "$runtime_root"
    fail "successor backup failure probe 1"
  fi
  # replace only the three reviewed files
  while read -r _ _ _ mode relative; do
    successor_changed_path "$relative" || continue
    successor_install_control_file "$candidate/$relative" "$destination/$relative" "$mode" "$uid" "$gid"
    replacement_step=$((replacement_step + 1))
    if ((failure_step == replacement_step + 1)); then
      successor_failure_probe "$failure_step" "$runtime_root"
      fail "successor replacement failure probe $failure_step"
    fi
  done < <(predecessor_identity_manifest)
  successor_verify_live_candidate_identities "$destination" "$candidate_manifest" "$uid" "$gid"
  [[ "$(control_digest "$destination")" == "$candidate_digest" ]] || fail "installed successor digest differs"
  [[ "$(successor_state_snapshot_sha256 "$destination" "$runtime_root" "$uid" "$gid" "$data_uid" "$data_gid")" == "$snapshot_sha" ]] ||
    fail "source state changed during successor installation"
  successor_publish_handoff "$receipt" "$candidate_digest" "$candidate_manifest_sha" "$uid" "$gid"
  if ((failure_step == 5)); then
    successor_failure_probe 5 "$runtime_root"
    fail "successor handoff failure probe 5"
  fi
  transaction_started=0
  rm -rf -- "$stage"; stage=''
  trap - EXIT
  printf 'Installed exact v16 successor control plane: %s\nRetained successor recovery: %s\n' \
    "$candidate_digest" "$backup"
)

# dispatch only fixed production install or distinct recovery
successor_main() {
  ((EUID == 0)) || fail "host administrator authority is required"
  # keep recovery distinct from the frozen v15 --recover contract
  if (($# == 2)) && [[ "$1" == --recover-successor-v1 ]]; then
    recover_successor_control_plane_v1 /opt/weather/current /var/lib/weather/control-plane-backups \
      /var/lib/weather "$2" 0 0 10002 10002
    return
  fi
  (($# == 3)) || fail "usage: install-adjustment-recurring-control-plane-successor-v1.sh ARCHIVE ARCHIVE_SHA256 CANDIDATE_SHA256 | --recover-successor-v1 BACKUP"
  install_successor_control_plane_v1 /opt/weather/current /var/lib/weather/control-plane-backups \
    /var/lib/weather "$1" "$2" "$3" 0 0 0 10002 10002
}

# run only when directly invoked
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  successor_main "$@"
fi
