#!/usr/bin/env bash
set -euo pipefail

# resume above the published high code after the rejected reset in run seven
baseline_code=262850157
baseline_run=7
run_number="${GITHUB_RUN_NUMBER:-}"

# reject absent or unsafe counters before bounded decimal arithmetic
if [[ ! "${run_number}" =~ ^[1-9][0-9]{0,9}$ ]]; then
  printf 'GITHUB_RUN_NUMBER must be a positive decimal workflow counter\n' >&2
  exit 1
fi
run_number_number="$((10#${run_number}))"

# reject allocation runs from before the restored counter
if (( run_number_number <= baseline_run )); then
  printf 'Android release must use a workflow run newer than run %s\n' "${baseline_run}" >&2
  exit 1
fi
version_code_number="$((baseline_code + run_number_number - baseline_run))"

# stay within google play's positive version-code range
if (( version_code_number > 2100000000 )); then
  printf 'derived Android version code is outside the Play range\n' >&2
  exit 1
fi

printf '%s\n' "${version_code_number}"
