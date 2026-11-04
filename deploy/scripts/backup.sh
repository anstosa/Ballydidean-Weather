#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# print backup usage
usage() {
  cat <<'EOF'
Usage: backup.sh [--recipient AGE_RECIPIENT] [--output-dir DIRECTORY] [--env-file FILE] [--max-output-bytes BYTES]

Streams a PostgreSQL custom-format dump directly into age encryption, then
atomically publishes the encrypted artifact and adjacent SHA-256 checksum.
No plaintext database dump is written.

--max-output-bytes enables the activation-only bounded writer. The scheduled
backup default remains uncapped.
EOF
}

# write encrypted bytes without crossing one explicit ceiling
bounded_encrypted_writer() {
  local target=$1
  local maximum=$2
  local probe

  # preserve a failed or short bounded write even inside a conditional pipeline
  dd bs=1M iflag=count_bytes count="$maximum" of="$target" status=none || return $?
  # detect one excess byte without writing it
  probe=$(LC_ALL=C od -An -N1 -tu1) || return $?
  if [[ "$probe" =~ [0-9] ]]; then
    return 90
  fi
}

recipient=${AGE_RECIPIENT:-}
output_dir=/var/lib/weather/backups
WEATHER_ENV_FILE=${WEATHER_ENV_FILE:-$(default_env_file)}
max_output_bytes=

# parse backup options
while (($# > 0)); do
  case "$1" in
    --recipient)
      (($# >= 2)) || die "--recipient requires a value"
      recipient=$2
      shift 2
      ;;
    --output-dir)
      (($# >= 2)) || die "--output-dir requires a value"
      output_dir=$2
      shift 2
      ;;
    --env-file)
      (($# >= 2)) || die "--env-file requires a value"
      WEATHER_ENV_FILE=$2
      shift 2
      ;;
    --max-output-bytes)
      (($# >= 2)) || die "--max-output-bytes requires a value"
      max_output_bytes=$2
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *) die "unknown argument: $1" ;;
  esac
done

# load public backup policy
if [[ -z "$recipient" && -f "$deploy_dir/config/backup.env" ]]; then
  recipient=$(env_value "$deploy_dir/config/backup.env" AGE_RECIPIENT)
fi

[[ "$recipient" == age1* || "$recipient" == ssh-* ]] ||
  die "set a valid age or SSH recipient"
[[ "$output_dir" == /* ]] || die "output directory must be absolute"
[[ -z "$max_output_bytes" ||
  ( "$max_output_bytes" =~ ^[1-9][0-9]*$ && ${#max_output_bytes} -le 18 ) ]] ||
  die "maximum output bytes must be a positive bounded integer"
require_command age
require_command docker
require_command sha256sum
require_command sync
# load bounded-writer utilities only when requested
if [[ -n "$max_output_bytes" ]]; then
  require_command dd
  require_command od
fi
require_file "$WEATHER_ENV_FILE"
database_name=$(env_value "$WEATHER_ENV_FILE" WEATHER_DATABASE_NAME)
validate_database_name "$database_name"

umask 077
mkdir -p "$output_dir"
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
temporary=$(mktemp "$output_dir/.weather-${timestamp}.XXXXXX.dump.age.partial")
nonce=$(basename "$temporary" | sed -E 's/^\.weather-[^.]+\.([^.]+)\.dump\.age\.partial$/\1/')
archive="$output_dir/weather-${timestamp}-${nonce}.dump.age"
checksum="$archive.sha256"
checksum_temporary="$checksum.partial"
publication_complete=false

# remove interrupted publications
cleanup() {
  rm -f "$temporary" "$checksum_temporary"

  # remove an incomplete published pair
  if [[ "$publication_complete" != true ]]; then
    rm -f "$archive" "$checksum"
  fi
}
trap cleanup EXIT

printf 'Creating encrypted PostgreSQL backup %s\n' "$archive"
# preserve the scheduled direct writer unless activation supplies a ceiling
if [[ -n "$max_output_bytes" ]]; then
  if ! WEATHER_ENV_FILE=$WEATHER_ENV_FILE compose exec -T postgres \
    pg_dump --username weather_owner --dbname "$database_name" --format=custom --no-owner |
    age --recipient "$recipient" |
    bounded_encrypted_writer "$temporary" "$max_output_bytes"; then
    die "bounded encrypted backup failed or exceeded its byte ceiling"
  fi
else
  WEATHER_ENV_FILE=$WEATHER_ENV_FILE compose exec -T postgres \
    pg_dump --username weather_owner --dbname "$database_name" --format=custom --no-owner |
    age --recipient "$recipient" --output "$temporary"
fi
[[ -s "$temporary" ]] || die "encrypted backup is empty"

digest=$(sha256sum "$temporary" | awk '{print $1}')
printf '%s  %s\n' "$digest" "$(basename "$archive")" >"$checksum_temporary"
chmod 600 "$temporary" "$checksum_temporary"
sync -f "$temporary" "$checksum_temporary"
mv "$temporary" "$archive"
mv "$checksum_temporary" "$checksum"
sync -f "$output_dir"
publication_complete=true
trap - EXIT
printf 'Backup complete: %s\nChecksum: %s\n' "$archive" "$checksum"
