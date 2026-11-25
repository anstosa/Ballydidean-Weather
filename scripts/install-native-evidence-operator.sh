#!/usr/bin/env bash
set -euo pipefail

# install one immutable native host application package while preserving the predecessor gateway
(($# == 4)) || { printf 'usage: install-native-evidence-operator.sh PACKAGE BUNDLE_SHA256 ACTIVE_GATEWAY_SHA256 LEGACY_GATEWAY_SHA256\n' >&2; exit 2; }
[[ "$EUID" == 0 ]] || { printf 'error: native evidence installation requires root\n' >&2; exit 1; }
source_dir=$1
bundle=$2
active_sha=$3
legacy_sha=$4
root=/opt/weather/operators/native-evidence
gateway=/usr/local/sbin/weather-remote-ops

# report installation failures without changing unrelated host paths
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

# retain only the supplied exact content addresses
for identity in "$bundle" "$active_sha" "$legacy_sha"; do
  [[ "$identity" =~ ^[a-f0-9]{64}$ ]] || fail "native evidence identity differs"
done
[[ "$source_dir" == /* && -d "$source_dir" && ! -L "$source_dir" && "$(realpath -e "$source_dir")" == "$source_dir" &&
  "$(stat -c '%u:%g:%a' "$source_dir")" == 0:0:700 ]] || fail "native evidence source directory differs"
[[ "$(find "$source_dir" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)" == \
  $'manifest.sha256\nnative-dashboard-release-control.sh\nnative-evidence-remote-dispatch.sh\nnative-generation-remote-ops.sh\nnative-release-control-remote-dispatch.sh\nsource-commit\nweather-evidence' ]] ||
  fail "native evidence package members differ"

# hash one exact file with its expected installed mode
file_sha() {
  local path=$1 mode=$2
  [[ -f "$path" && ! -L "$path" && "$(realpath -e "$path")" == "$path" &&
    "$(stat -c '%u:%g:%a:%h' "$path")" == "0:0:$mode:1" ]] || fail "native evidence file metadata differs"
  sha256sum "$path" | awk '{print $1}'
}

# verify the complete closed package without consuming arbitrary manifest paths
package_sha() {
  local directory=$1 member mode manifest=
  for member in native-dashboard-release-control.sh native-evidence-remote-dispatch.sh native-generation-remote-ops.sh native-release-control-remote-dispatch.sh source-commit weather-evidence; do
    mode=644
    [[ "$member" != weather-evidence && "$member" != native-generation-remote-ops.sh ]] || mode=755
    manifest+="$(file_sha "$directory/$member" "$mode")  $member"$'\n'
  done
  [[ "$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')" == "$(file_sha "$directory/manifest.sha256" 644)" ]] ||
    fail "native evidence manifest differs"
  printf '%s' "$manifest" | sha256sum | awk '{print $1}'
}

[[ "$(package_sha "$source_dir")" == "$bundle" ]] || fail "native evidence bundle differs"
commit=$(cat "$source_dir/source-commit")
[[ "$commit" =~ ^[a-f0-9]{40}$ ]] || fail "native evidence source commit differs"
[[ "$(uname -m)" == aarch64 ]] || fail "native evidence host architecture differs"
# check native ELF64 little-endian AArch64 with the host's existing coreutils
[[ "$(od -An -v -tx1 -N7 "$source_dir/weather-evidence" | tr -d ' \n')" == 7f454c46020101 &&
  "$(od -An -v -tx1 -j18 -N2 "$source_dir/weather-evidence" | tr -d ' \n')" == b700 ]] ||
  fail "native evidence executable architecture differs"
actual_gateway=$(file_sha "$gateway" 755)
new_gateway=$(file_sha "$source_dir/native-generation-remote-ops.sh" 755)
[[ "$actual_gateway" == "$active_sha" || "$actual_gateway" == "$new_gateway" ]] ||
  fail "active remote gateway differs"

# create only this application operator namespace and immutable package collections
for directory in "$root" "$root/packages" "$root/by-commit" "$root/gateways"; do
  if [[ ! -e "$directory" && ! -L "$directory" ]]; then install -d -m 0755 -o 0 -g 0 "$directory"; fi
  [[ -d "$directory" && ! -L "$directory" && "$(realpath -e "$directory")" == "$directory" &&
    "$(stat -c '%u:%g:%a' "$directory")" == 0:0:755 ]] || fail "native evidence installation directory differs"
done

# serialize the bounded gateway/install handoff independently of application operations
exec 9>"$root/.install.lock"
flock -x 9
# recheck the active gateway after acquiring the install lock
[[ "$(file_sha "$gateway" 755)" == "$actual_gateway" ]] || fail "active remote gateway changed"

# retain the original Node gateway and its immutable receipt
legacy="$root/legacy-remote-ops.sh"
legacy_receipt="$root/legacy-remote-ops.sha256"
if [[ ! -e "$legacy" && ! -L "$legacy" ]]; then
  [[ "$actual_gateway" == "$legacy_sha" && ! -e "$legacy_receipt" && ! -L "$legacy_receipt" ]] ||
    fail "legacy remote gateway predecessor differs"
  temporary=$(mktemp "$root/.legacy.XXXXXX")
  install -m 0755 -o 0 -g 0 "$gateway" "$temporary"
  mv -T -- "$temporary" "$legacy"
fi
# recover a crash between predecessor byte publication and receipt publication
if [[ ! -e "$legacy_receipt" && ! -L "$legacy_receipt" ]]; then
  [[ "$actual_gateway" == "$legacy_sha" && "$(file_sha "$legacy" 755)" == "$legacy_sha" ]] ||
    fail "legacy gateway receipt predecessor differs"
  temporary=$(mktemp "$root/.legacy-receipt.XXXXXX")
  printf '%s\n' "$legacy_sha" >"$temporary"
  chmod 0600 "$temporary"
  mv -T -- "$temporary" "$legacy_receipt"
fi
[[ "$(file_sha "$legacy" 755)" == "$legacy_sha" && "$(file_sha "$legacy_receipt" 600)" == "$(printf '%s\n' "$legacy_sha" | sha256sum | awk '{print $1}')" ]] ||
  fail "retained legacy gateway differs"
retained="$root/gateways/$actual_gateway"
if [[ ! -e "$retained" && ! -L "$retained" ]]; then
  temporary=$(mktemp "$root/gateways/.retain.XXXXXX")
  install -m 0755 -o 0 -g 0 "$gateway" "$temporary"
  mv -T -- "$temporary" "$retained"
fi
[[ "$(file_sha "$retained" 755)" == "$actual_gateway" ]] || fail "retained active gateway differs"

# publish immutable packages and source-commit mapping before switching the gateway
target="$root/packages/$bundle"
if [[ ! -e "$target" && ! -L "$target" ]]; then
  temporary=$(mktemp -d "$root/packages/.install.XXXXXX")
  for member in native-dashboard-release-control.sh native-evidence-remote-dispatch.sh native-generation-remote-ops.sh native-release-control-remote-dispatch.sh source-commit weather-evidence manifest.sha256; do
    mode=644
    [[ "$member" != weather-evidence && "$member" != native-generation-remote-ops.sh ]] || mode=755
    install -m "$mode" -o 0 -g 0 "$source_dir/$member" "$temporary/$member"
  done
  chmod 0755 "$temporary"
  [[ "$(package_sha "$temporary")" == "$bundle" ]] || fail "published native evidence package differs"
  mv -T -- "$temporary" "$target"
fi
[[ "$(package_sha "$target")" == "$bundle" ]] || fail "retained native evidence package differs"
pointer="$root/by-commit/$commit"
if [[ ! -e "$pointer" && ! -L "$pointer" ]]; then
  temporary=$(mktemp "$root/by-commit/.mapping.XXXXXX")
  printf '%s\n' "$bundle" >"$temporary"
  chmod 0600 "$temporary"
  mv -T -- "$temporary" "$pointer"
fi
[[ "$(file_sha "$pointer" 600)" == "$(printf '%s\n' "$bundle" | sha256sum | awk '{print $1}')" ]] ||
  fail "retained native evidence commit mapping differs"
sync -f "$target" "$pointer" "$root"

# replace only the fixed installed gateway with an atomic rename
if [[ "$actual_gateway" != "$new_gateway" ]]; then
  temporary=$(mktemp /usr/local/sbin/.weather-remote-ops.XXXXXX)
  install -m 0755 -o 0 -g 0 "$target/native-generation-remote-ops.sh" "$temporary"
  mv -T -- "$temporary" "$gateway"
  sync -f "$gateway" /usr/local/sbin
fi
# verify the installed package through the real gateway, restoring the exact previous bytes on failure
if ! "$gateway" --verify-native-package "$commit"; then
  temporary=$(mktemp /usr/local/sbin/.weather-remote-ops.restore.XXXXXX)
  install -m 0755 -o 0 -g 0 "$retained" "$temporary"
  mv -T -- "$temporary" "$gateway"
  sync -f "$gateway" /usr/local/sbin
  [[ "$(file_sha "$gateway" 755)" == "$actual_gateway" ]] || fail "native gateway restoration differs"
  fail "native evidence package verification failed"
fi
printf 'Installed native evidence package %s for %s; predecessor gateway retained.\n' "$bundle" "$commit"
