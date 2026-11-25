#!/usr/bin/env bash
set -euo pipefail

# build one separate ARM64 host package from the exact native build artifact and checkout
(($# == 3)) || { printf 'usage: package-native-evidence-operator.sh BINARY OUTPUT SOURCE_COMMIT\n' >&2; exit 2; }
binary=$(realpath -e -- "$1")
output=$2
commit=$3
[[ "$commit" =~ ^[a-f0-9]{40}$ && "$commit" == "$(git rev-parse HEAD)" && -f "$binary" && -x "$binary" ]] ||
  { printf 'error: native host build source differs\n' >&2; exit 1; }
# require the shipped executable to match the existing Blueberry native target
LC_ALL=C readelf -h "$binary" | grep -Eq 'Machine:[[:space:]]+AArch64$'
[[ ! -e "$output" && ! -L "$output" ]] || { printf 'error: native host output already exists\n' >&2; exit 1; }
mkdir -m 0755 -- "$output"
install -m 0755 -- "$binary" "$output/weather-evidence"
# retain the exact checked out dispatcher and gateway bytes without compiling them into either image
for member in native-dashboard-release-control.sh native-evidence-remote-dispatch.sh native-generation-remote-ops.sh native-release-control-remote-dispatch.sh; do
  path="deploy/scripts/$member"
  [[ "$(git show "$commit:$path" | sha256sum | awk '{print $1}')" == "$(sha256sum "$path" | awk '{print $1}')" ]] ||
    { printf 'error: native host dispatch source differs\n' >&2; exit 1; }
  mode=0644
  [[ "$member" != native-generation-remote-ops.sh ]] || mode=0755
  install -m "$mode" -- "$path" "$output/$member"
done
printf '%s\n' "$commit" >"$output/source-commit"
chmod 0644 "$output/source-commit"
(
  cd "$output"
  sha256sum native-dashboard-release-control.sh native-evidence-remote-dispatch.sh native-generation-remote-ops.sh native-release-control-remote-dispatch.sh source-commit weather-evidence >manifest.sha256
  chmod 0644 manifest.sha256
  sha256sum manifest.sha256
)
