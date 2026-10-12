#!/usr/bin/env bash
set -euo pipefail

# anchor the counter above the last timestamp-based release from workflow run five
last_timestamp_code=262850156
last_timestamp_run=5
run_number="${GITHUB_RUN_NUMBER:-}"

# reject absent or unsafe counters before bounded decimal arithmetic
if [[ ! "${run_number}" =~ ^[1-9][0-9]{0,9}$ ]]; then
  printf 'GITHUB_RUN_NUMBER must be a positive decimal workflow counter\n' >&2
  exit 1
fi
run_number_number="$((10#${run_number}))"

# never generate a code at or below the published migration baseline
if (( run_number_number <= last_timestamp_run )); then
  printf 'Android release must use a workflow run newer than run %s\n' "${last_timestamp_run}" >&2
  exit 1
fi
version_code_number="$((last_timestamp_code + run_number_number - last_timestamp_run))"

# stay within google play's positive version-code range
if (( version_code_number > 2100000000 )); then
  printf 'derived Android version code is outside the Play range\n' >&2
  exit 1
fi

printf '%s\n' "${version_code_number}"
