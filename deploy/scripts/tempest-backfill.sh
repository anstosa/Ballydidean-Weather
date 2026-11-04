#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# require the root-owned operator boundary
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  die "Tempest backfill must run through sudo"
fi

(($# == 0)) || die "Tempest backfill takes no arguments"
require_command docker
acquire_weather_lifecycle_lock

current=$(read_release_state "$deploy_dir/state/current-release")
current_env="$deploy_dir/releases/$current.env"
require_file "$current_env"
report_directory=/var/lib/weather
report="$report_directory/tempest-backfill-$current.json"
mkdir -p "$report_directory"
umask 077
partial=$(mktemp "$report_directory/.tempest-backfill-$current.XXXXXX.partial")

# remove an interrupted report
trap 'rm -f "$partial"' EXIT

# import every configured station from its checked history start
WEATHER_ENV_FILE="$current_env" compose run --rm --no-deps worker \
  node apps/worker/dist/tempest-backfill-cli.js \
  --site ballydidean --resume >"$partial"

chmod 600 "$partial"
mv "$partial" "$report"
trap - EXIT
printf 'Tempest historical import completed for %s\nReport: %s\n' "$current" "$report"
