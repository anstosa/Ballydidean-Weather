#!/usr/bin/env bash

# report native evidence ownership
native_evidence_handles() {
  local action=${1-}
  # keep the installed ssh surface closed
  case "$action" in
    adjustment-family-release-status|adjustment-family-release-current-v1|adjustment-family-release-lineage-v2|\
    adjustment-revision-capture-epoch-v1|adjustment-revision-capture-epoch-snapshot-v1|\
    adjustment-shadow-metadata-custody-status-v1|\
    adjustment-revision-custody-ack-v1|adjustment-revision-custody-ack-v2|\
    adjustment-revision-gap-start-v1|adjustment-revision-gap-page-v1|adjustment-revision-gap-ack-v1|\
    adjustment-maintenance-anchor-status-v2|\
    adjustment-development-custody-anchor-install-v1|adjustment-development-custody-anchor-current-v1|\
    adjustment-rain-control-custody-anchor-install-v1|adjustment-rain-control-custody-anchor-current-v1|\
    adjustment-unsupported-terminal-proof-install-v1|adjustment-unsupported-terminal-proof-current-v1|\
    adjustment-future-input-seal-current-v2|adjustment-future-input-seal-install-v2|\
    adjustment-maintenance-anchor-current-v3|adjustment-maintenance-anchor-install-v2|\
    adjustment-maintenance-anchor-install-v3|adjustment-maintenance-anchor-finalize-v2|\
    adjustment-maintenance-anchor-finalize-v3|adjustment-archive-next|adjustment-archive-ack|\
    adjustment-archive-ack-final|adjustment-family-release|\
    adjustment-registration-schedule-initialize-v3|adjustment-registration-schedule-status-v3|\
    adjustment-registration-lifecycle-status-v4|adjustment-shadow-metadata-custody-finalize-v1|\
    adjustment-confirmation-access-burn-v3|adjustment-shadow-terminal-record-v3|\
    adjustment-shadow-terminal-retire-v3|adjustment-shadow-unsupported-terminal-record-v1|\
    adjustment-shadow-unsupported-terminal-retire-v1|adjustment-database-ledger-v3|\
    adjustment-revision-catalog-current-start-v1|adjustment-revision-catalog-start-v1|\
    adjustment-revision-catalog-page-v1|adjustment-evaluation-export|\
    adjustment-evaluation-export-v2|adjustment-confirmation-availability-v2|\
    adjustment-confirmation-export-v2|forecast-training-export|install-adjustment-scorecard|install-adjustment-scorecard-v2)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# execute one root-owned command
native_evidence_remote_exec_root() {
  local binary=${native_evidence_binary-}
  exec "$binary" "$@"
}

# execute with the original service identity
native_evidence_remote_exec_service() {
  local binary=${native_evidence_binary-}
  exec setpriv --reuid=10002 --regid=10002 --clear-groups -- \
    "$binary" "$@"
}

# validate one sha-256 operand
native_evidence_remote_is_sha256() {
  [[ ${1-} =~ ^[a-f0-9]{64}$ ]]
}

# validate one postgres bigint
native_evidence_remote_is_bigint() {
  local value=${1-}
  [[ $value =~ ^(0|[1-9][0-9]{0,18})$ ]] || return 1
  # avoid signed shell overflow
  # shellcheck disable=SC2071
  [[ ${#value} -lt 19 || $value < 9223372036854775808 ]]
}

# validate admitted anchor authority
native_evidence_remote_has_anchor_authority() {
  [[ ${WEATHER_ANCHOR_CONTROL_SHA256-} =~ ^[a-f0-9]{64}$ &&
    ${WEATHER_ANCHOR_CONTROL_VERSION-} =~ ^[1-9][0-9]{0,5}$ ]]
}

# refuse malformed mapped operands
native_evidence_remote_invalid_arguments() {
  printf 'error: invalid native evidence arguments\n' >&2
  return 2
}

# stream the original read-only forecast training transaction through the native archive producer
native_host_forecast_training_export() (
max_days=450
max_rows=4000000
conservative_export_rows=3045600
export_row_headroom=954400

from_date=$1
to_date=$2
validate_calendar_date_range "$from_date" "$to_date" "$max_days"

# require the root-owned forced operation
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  die "forecast-training export must run through sudo"
fi

# freeze the reviewed capacity oracle
if ((conservative_export_rows + export_row_headroom != max_rows)); then
  die "export capacity oracle is inconsistent"
fi

require_command docker
require_command gzip
# the authenticated gateway selects the exact committed environment
# shellcheck disable=SC2154
WEATHER_ENV_FILE=$native_active_environment
require_file "$WEATHER_ENV_FILE"
database_name=$(env_value "$WEATHER_ENV_FILE" WEATHER_DATABASE_NAME)
validate_database_name "$database_name"
temporary=$(mktemp -d /tmp/weather-forecast-training.XXXXXXXX)
transaction_stream="$temporary/transaction.jsonl.partial"
query_errors="$temporary/query.stderr"

# remove every server-side temporary
# shellcheck disable=SC2329
cleanup() {
  rm -rf -- "$temporary"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

cat >"$temporary/export.sql" <<'SQL'
\set ON_ERROR_STOP on
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '15min';
SET LOCAL lock_timeout = '5s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
SET LOCAL weather.forecast_training_from_date TO :'from_date';
SET LOCAL weather.forecast_training_to_date TO :'to_date';
COPY (
  WITH requested AS (
    SELECT
      :'from_date'::date AS from_date,
      :'to_date'::date AS to_date,
      (:'from_date'::date::timestamp AT TIME ZONE 'America/Los_Angeles') AS start_inclusive,
      ((:'to_date'::date + 1)::timestamp AT TIME ZONE 'America/Los_Angeles') AS end_exclusive
  ),
  bounded_rows AS (
    SELECT exported.*
    FROM forecast_training_export_rows_v1 exported
    CROSS JOIN requested
    WHERE exported.site_key = 'ballydidean'
      AND exported.valid_at >= requested.start_inclusive
      AND exported.valid_at < requested.end_exclusive
    ORDER BY
      exported.valid_at,
      exported.record_kind COLLATE "C",
      exported.physical_station_key COLLATE "C" NULLS FIRST,
      exported.reference_at NULLS FIRST,
      exported.target_lead_hours NULLS FIRST,
      exported.source_keys::text COLLATE "C",
      exported.content_hashes::text COLLATE "C"
    LIMIT 4000001
  ),
  records AS (
    SELECT
      0 AS record_order,
      NULL::timestamptz AS valid_order,
      NULL::text AS kind_order,
      NULL::text AS station_order,
      NULL::timestamptz AS reference_order,
      NULL::smallint AS lead_order,
      NULL::text AS source_order,
      NULL::text AS hash_order,
      jsonb_build_object(
        'record_type', 'manifest',
        'payload', to_jsonb(manifest),
        'transaction', jsonb_build_object(
          'created_at_utc', to_char(
            transaction_timestamp() AT TIME ZONE 'UTC',
            'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
          ),
          'from_local_date', requested.from_date::text,
          'idle_in_transaction_session_timeout',
            current_setting('idle_in_transaction_session_timeout'),
          'isolation_level', current_setting('transaction_isolation'),
          'lock_timeout', current_setting('lock_timeout'),
          'read_only', current_setting('transaction_read_only'),
          'statement_timeout', current_setting('statement_timeout'),
          'to_local_date', requested.to_date::text
        )
      )::text AS line
    FROM forecast_training_export_manifest_v1 manifest
    CROSS JOIN requested

    UNION ALL

    SELECT
      1,
      exported.valid_at,
      exported.record_kind,
      exported.physical_station_key,
      exported.reference_at,
      exported.target_lead_hours,
      exported.source_keys::text,
      exported.content_hashes::text,
      jsonb_build_object(
        'record_type', 'row',
        'payload',
          to_jsonb(exported)
          || jsonb_build_object(
            'received_at', CASE
              WHEN exported.received_at IS NULL THEN NULL
              ELSE to_char(
                exported.received_at AT TIME ZONE 'UTC',
                'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
              )
            END,
            'reference_at', CASE
              WHEN exported.reference_at IS NULL THEN NULL
              ELSE to_char(
                exported.reference_at AT TIME ZONE 'UTC',
                'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
              )
            END,
            'valid_at', to_char(
              exported.valid_at AT TIME ZONE 'UTC',
              'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
            )
          )
      )::text
    FROM bounded_rows exported
  )
  SELECT line
  FROM records
  ORDER BY
    record_order,
    valid_order,
    kind_order COLLATE "C" NULLS FIRST,
    station_order COLLATE "C" NULLS FIRST,
    reference_order NULLS FIRST,
    lead_order NULLS FIRST,
    source_order COLLATE "C" NULLS FIRST,
    hash_order COLLATE "C" NULLS FIRST
) TO STDOUT;
COMMIT;
SQL

# run the fixed query through only the read-only role
# shellcheck disable=SC2016
if ! WEATHER_ENV_FILE=$WEATHER_ENV_FILE compose exec -T postgres \
  sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_training_export_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --host 127.0.0.1 --username weather_training_export --dbname "$1" --set=from_date="$2" --set=to_date="$3"' \
  weather-training-export "$database_name" "$from_date" "$to_date" \
  <"$temporary/export.sql" >"$transaction_stream" 2>"$query_errors"; then
  die "read-only training export query failed"
fi


# write the exact deterministic compressed stream only after complete native package verification
"$native_evidence_binary" stream-forecast-training-export "$from_date" "$to_date" "$temporary/package" <"$transaction_stream"
)

# prepare one fixed read-only export envelope
native_evidence_remote_prepare_export() {
  local require_evidence=$1
  local lock_root=/run/lock/weather
  local temporary_root=/var/lib/weather/xweather/adjustment-evaluation-exports
  local free_bytes free_inodes

  # require the original root-owned auxiliary context
  [[ ${EUID:-$(id -u)} -eq 0 && ${native_active_environment-} == /* ]] || {
    printf 'error: native evidence export context is unavailable\n' >&2
    return 69
  }
  require_command df || return
  require_command docker || return
  require_command flock || return
  require_file "$native_active_environment" || return
  native_evidence_export_environment=$native_active_environment
  native_evidence_export_database=$(
    env_value "$native_evidence_export_environment" WEATHER_DATABASE_NAME
  ) || return
  validate_database_name "$native_evidence_export_database" || return

  # require the fixed evidence root for cold archive operations
  if [[ $require_evidence == 1 ]]; then
    [[ -d /var/lib/weather/xweather/adjustment-evidence &&
      ! -L /var/lib/weather/xweather/adjustment-evidence ]] || {
      printf 'error: adjustment evidence root is missing or linked\n' >&2
      return 1
    }
  fi

  # serialize the reservation and complete native projection
  install -d -o 0 -g 0 -m 0700 "$lock_root" || return
  [[ ! -L $lock_root ]] || {
    printf 'error: adjustment export lock root is linked\n' >&2
    return 1
  }
  exec {native_evidence_export_lock_fd}>"$lock_root/adjustment-evaluation-export.lock" || return
  chmod 0600 "$lock_root/adjustment-evaluation-export.lock" || return
  flock -n "$native_evidence_export_lock_fd" || {
    printf 'error: another adjustment evaluation export is in flight\n' >&2
    return 1
  }

  install -d -m 0700 "$temporary_root" || return
  [[ ! -L $temporary_root ]] || {
    printf 'error: adjustment export temporary root is linked\n' >&2
    return 1
  }

  # admit the original byte and inode reservation
  read -r free_bytes free_inodes < <(
    printf '%s %s\n' \
      "$(df --output=avail --block-size=1 "$temporary_root" | tail -n 1 | tr -d ' ')" \
      "$(df --output=iavail "$temporary_root" | tail -n 1 | tr -d ' ')"
  )
  [[ $free_bytes =~ ^[0-9]+$ && $free_inodes =~ ^[0-9]+$ ]] || {
    printf 'error: filesystem capacity could not be measured\n' >&2
    return 1
  }
  # retain the fixed 64 mib reservation and 1.75 gib floor
  if ((free_bytes - 67108864 < 1879048192 + 16777216)); then
    printf 'error: adjustment export reservation would cross the fixed filesystem floor\n' >&2
    return 1
  fi
  # retain the fixed inode floor
  if ((free_inodes < 32768)); then
    printf 'error: adjustment export requires at least 32768 free inodes\n' >&2
    return 1
  fi

  native_evidence_export_temporary=$(mktemp -d "$temporary_root/.export.XXXXXXXX") || return
}

# remove only the current fixed export envelope
native_evidence_remote_cleanup_export() {
  [[ -n ${native_evidence_export_temporary-} ]] || return 0
  rm -rf -- "$native_evidence_export_temporary"
}

# write the original function-only repeatable-read query
native_evidence_remote_write_function_export_sql() {
  local function_expression=$1
  cat >"$native_evidence_export_temporary/function-export.sql" <<SQL
\\set ON_ERROR_STOP on
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '5min';
SET LOCAL lock_timeout = '5s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
COPY (
  SELECT jsonb_build_object(
    'databaseManifest', to_jsonb(manifest),
    'payload', $function_expression,
    'transaction', jsonb_build_object(
      'created_at_utc', to_char(
        transaction_timestamp() AT TIME ZONE 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
      ),
      'idle_in_transaction_session_timeout',
        current_setting('idle_in_transaction_session_timeout'),
      'isolation_level', current_setting('transaction_isolation'),
      'lock_timeout', current_setting('lock_timeout'),
      'read_only', current_setting('transaction_read_only'),
      'statement_timeout', current_setting('statement_timeout')
    )
  )::text
  FROM adjustment_evaluation_export_manifest_v1 manifest
) TO STDOUT WITH (
  FORMAT csv,
  DELIMITER E'\\x01',
  QUOTE E'\\x02',
  ESCAPE E'\\x02'
);
COMMIT;
SQL
}

# run the original training-role function export
native_evidence_remote_run_function_export() {
  local registration_sha256=${1-}
  local access_sha256=${2-}
  local chunk_index=${3-}
  local revision_cutoff=${4-}
  local revision_ordinal=${5-}
  # keep the password inside the fixed postgres container
  # shellcheck disable=SC2016
  WEATHER_ENV_FILE=$native_evidence_export_environment compose exec -T postgres \
    sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_training_export_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --host 127.0.0.1 --username weather_training_export --dbname "$1" --set=registration_sha256="$2" --set=access_sha256="$3" --set=chunk_index="$4" --set=revision_cutoff="$5" --set=revision_ordinal="$6"' \
    adjustment-maintenance-v2-export "$native_evidence_export_database" \
    "$registration_sha256" "$access_sha256" "$chunk_index" \
    "$revision_cutoff" "$revision_ordinal" \
    <"$native_evidence_export_temporary/function-export.sql"
}

# project the authenticated database ledger through the native codec
native_evidence_remote_run_database_ledger() (
  set -o pipefail
  native_evidence_remote_prepare_export 0 || return
  trap native_evidence_remote_cleanup_export EXIT
  trap 'exit 130' HUP INT TERM
  native_evidence_remote_write_function_export_sql "NULL::jsonb" || return
  # reject either query or native projection failure
  if ! native_evidence_remote_run_function_export | \
    native_evidence_remote_exec_root project-maintenance-ledger-v3; then
    printf 'error: adjustment database ledger export failed\n' >&2
    return 1
  fi
)

# project the fixed family schedule through the native codec
native_evidence_remote_run_registration_schedule() (
  set -o pipefail
  native_evidence_remote_prepare_export 1 || return
  trap native_evidence_remote_cleanup_export EXIT
  trap 'exit 130' HUP INT TERM
  native_evidence_remote_write_function_export_sql \
    "jsonb_build_array(adjustment_shadow_registration_slot_v3('temperature'), adjustment_shadow_registration_slot_v3('wind'), adjustment_shadow_registration_slot_v3('rain'))" || return
  # reject either query or native projection failure
  if ! native_evidence_remote_run_function_export | \
    native_evidence_remote_exec_root project-registration-schedule-status-v3; then
    printf 'error: adjustment registration schedule status failed\n' >&2
    return 1
  fi
)

# project one immutable revision snapshot through the native cold-start owner
native_evidence_remote_run_revision_start() (
  local revision_cutoff=$1
  local revision_ordinal=${2-}
  local current=$3
  local function_expression
  set -o pipefail
  native_evidence_remote_prepare_export 1 || return
  trap native_evidence_remote_cleanup_export EXIT
  trap 'exit 130' HUP INT TERM

  # select only the current-frontier or explicit-watermark function
  if [[ $current == 1 ]]; then
    function_expression="(SELECT adjustment_revision_serving_snapshot_v1(:'revision_cutoff'::timestamptz, (observed.frontier->>'archiveCommitOrdinal')::bigint) FROM adjustment_revision_frontier_v1() AS observed(frontier))"
  else
    function_expression="adjustment_revision_serving_snapshot_v1(:'revision_cutoff'::timestamptz, :'revision_ordinal'::bigint)"
  fi
  native_evidence_remote_write_function_export_sql "$function_expression" || return
  # keep the cold archive root fixed
  if ! native_evidence_remote_run_function_export "" "" "" "$revision_cutoff" "$revision_ordinal" | \
    WEATHER_ADJUSTMENT_EVIDENCE_ROOT=/var/lib/weather/xweather/adjustment-evidence \
      native_evidence_remote_exec_root revision-cold-start-v1; then
    printf 'error: adjustment revision cold transfer start failed\n' >&2
    return 1
  fi
)

# read one direct-successor page from the fixed native cold spool
native_evidence_remote_run_revision_page() (
  set -o pipefail
  native_evidence_remote_prepare_export 1 || return
  trap native_evidence_remote_cleanup_export EXIT
  trap 'exit 130' HUP INT TERM
  # retain the outer cleanup owner while the child replaces only itself
  if ! (
    WEATHER_ADJUSTMENT_EVIDENCE_ROOT=/var/lib/weather/xweather/adjustment-evidence \
      native_evidence_remote_exec_root revision-cold-page-v1 "$@"
  ); then
    printf 'error: adjustment revision cold page export failed\n' >&2
    return 1
  fi
)

# project one value-free confirmation availability document
native_evidence_remote_run_confirmation_availability() (
  local registration_sha256=$1
  set -o pipefail
  native_evidence_remote_prepare_export 0 || return
  trap native_evidence_remote_cleanup_export EXIT
  trap 'exit 130' HUP INT TERM
  native_evidence_remote_write_function_export_sql \
    "adjustment_confirmation_availability_v2(:'registration_sha256')" || return
  # bind the exact function result to the forced registration
  if ! native_evidence_remote_run_function_export "$registration_sha256" | \
    native_evidence_remote_exec_root frame-v2-availability "$registration_sha256"; then
    printf 'error: value-free adjustment confirmation availability export failed\n' >&2
    return 1
  fi
)

# write the original bounded observation export query
native_evidence_remote_write_observation_sql() {
  cat >"$native_evidence_export_temporary/export.sql" <<'SQL'
\set ON_ERROR_STOP on
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '5min';
SET LOCAL lock_timeout = '5s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
SET LOCAL weather.adjustment_evaluation_from_date TO :'from_date';
SET LOCAL weather.adjustment_evaluation_to_date TO :'to_date';
COPY (
  WITH bounded_rows AS MATERIALIZED (
    SELECT exported.*
    FROM adjustment_evaluation_export_rows_v1 exported
    ORDER BY
      exported.local_date,
      exported.valid_at,
      exported.record_kind COLLATE "C",
      exported.record_revision_identity COLLATE "C"
    LIMIT 8193
  ),
  records AS (
    SELECT
      0 AS record_order,
      NULL::date AS local_date_order,
      NULL::timestamptz AS valid_order,
      NULL::text AS kind_order,
      NULL::text AS identity_order,
      jsonb_build_object(
        'record_type', 'manifest',
        'payload', to_jsonb(manifest),
        'transaction', jsonb_build_object(
          'created_at_utc', to_char(
            transaction_timestamp() AT TIME ZONE 'UTC',
            'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
          ),
          'from_local_date', :'from_date',
          'idle_in_transaction_session_timeout',
            current_setting('idle_in_transaction_session_timeout'),
          'isolation_level', current_setting('transaction_isolation'),
          'lock_timeout', current_setting('lock_timeout'),
          'read_only', current_setting('transaction_read_only'),
          'statement_timeout', current_setting('statement_timeout'),
          'to_local_date', :'to_date'
        )
      )::text AS line
    FROM adjustment_evaluation_export_manifest_v1 manifest

    UNION ALL

    SELECT
      1,
      exported.local_date,
      exported.valid_at,
      exported.record_kind,
      exported.record_revision_identity,
      jsonb_build_object(
        'record_type', 'row',
        'payload', jsonb_build_object(
          'compressed_body_base64', CASE
            WHEN exported.compressed_body IS NULL THEN NULL
            ELSE encode(exported.compressed_body, 'base64')
          END,
          'content_hash', exported.content_hash,
          'first_received_at', exported.first_received_at,
          'last_received_at', exported.last_received_at,
          'local_date', exported.local_date,
          'payload', exported.payload,
          'record_kind', exported.record_kind,
          'record_revision_identity', exported.record_revision_identity,
          'reference_at', exported.reference_at,
          'revision_count', exported.revision_count,
          'site_key', exported.site_key,
          'source_identity', exported.source_identity,
          'valid_at', exported.valid_at
        )
      )::text
    FROM bounded_rows exported
  )
  SELECT line
  FROM records
  ORDER BY
    record_order,
    local_date_order NULLS FIRST,
    valid_order NULLS FIRST,
    kind_order COLLATE "C" NULLS FIRST,
    identity_order COLLATE "C" NULLS FIRST
) TO STDOUT WITH (
  FORMAT csv,
  DELIMITER E'\x01',
  QUOTE E'\x02',
  ESCAPE E'\x02'
);
COMMIT;
SQL
}

# query the original bounded observation rows
native_evidence_remote_run_observation_query() {
  local from_date=$1
  local to_date=$2
  # keep the password inside the fixed postgres container
  # shellcheck disable=SC2016
  WEATHER_ENV_FILE=$native_evidence_export_environment compose exec -T postgres \
    sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_training_export_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --host 127.0.0.1 --username weather_training_export --dbname "$1" --set=from_date="$2" --set=to_date="$3"' \
    adjustment-evaluation-export "$native_evidence_export_database" \
    "$from_date" "$to_date" <"$native_evidence_export_temporary/export.sql"
}

# freeze the fixed evidence root into one private snapshot file
native_evidence_remote_freeze_snapshot() {
  local snapshot=$1
  # create the snapshot once with the source's private mode
  if ! (
    umask 077
    set -o noclobber
    WEATHER_ADJUSTMENT_EVIDENCE_ROOT=/var/lib/weather/xweather/adjustment-evidence \
      native_evidence_remote_exec_root freeze-adjustment-evidence-snapshot >"$snapshot"
  ); then
    printf 'error: adjustment evidence snapshot failed\n' >&2
    return 1
  fi
}

# stream one unchanged v1 observation archive
native_evidence_remote_stream_observation_archive() {
  local from_date=$1
  local to_date=$2
  local snapshot=$3
  native_evidence_remote_run_observation_query "$from_date" "$to_date" | \
    native_evidence_remote_exec_root stream-archive \
      "$native_evidence_export_temporary" "$from_date" "$to_date" "$snapshot"
}

# run one complete legacy, monitoring, or post-burn evaluation export
native_evidence_remote_run_evaluation_export() (
  local mode=$1
  local from_date=$2
  local to_date=$3
  local registration_sha256=${4-}
  local access_sha256=${5-}
  local chunk_index=${6-}
  local snapshot source_archive authorization_envelope
  set -o pipefail
  native_evidence_remote_prepare_export 1 || return
  trap native_evidence_remote_cleanup_export EXIT
  trap 'exit 130' HUP INT TERM

  # establish the schema-only precondition before observing evidence
  if [[ $mode == daily_v2 ]]; then
    native_evidence_remote_write_function_export_sql "NULL::jsonb" || return
    if ! native_evidence_remote_run_function_export | \
      native_evidence_remote_exec_root verify-v2-readiness; then
      printf 'error: adjustment maintenance v2 schema is not ready\n' >&2
      return 1
    fi
  fi

  # persist and validate the post-burn authorization before observation rows
  if [[ $mode == confirmation_v2 ]]; then
    authorization_envelope="$native_evidence_export_temporary/authorization-v2.json"
    native_evidence_remote_write_function_export_sql \
      "adjustment_confirmation_export_v2(:'registration_sha256', :'access_sha256', :'chunk_index'::smallint)" || return
    if ! native_evidence_remote_run_function_export \
      "$registration_sha256" "$access_sha256" "$chunk_index" | \
      native_evidence_remote_exec_root frame-v2-authorization \
        "$registration_sha256" "$access_sha256" "$chunk_index" \
        >"$authorization_envelope"; then
      printf 'error: post-burn adjustment confirmation authorization failed\n' >&2
      return 1
    fi
    # derive the exact inclusive archive dates from the persisted authorization
    if ! read -r from_date to_date < <(
      native_evidence_remote_exec_root v2-authorization-dates "$authorization_envelope"
    ); then
      printf 'error: post-burn adjustment confirmation dates failed\n' >&2
      return 1
    fi
    validate_calendar_date_range "$from_date" "$to_date" 14 || return
  fi

  snapshot="$native_evidence_export_temporary/edge-snapshot.json"
  native_evidence_remote_freeze_snapshot "$snapshot" || return
  native_evidence_remote_write_observation_sql || return

  # retain the byte-compatible v1 transport without staging it
  if [[ $mode == legacy_v1 ]]; then
    if ! native_evidence_remote_stream_observation_archive \
      "$from_date" "$to_date" "$snapshot"; then
      printf 'error: read-only adjustment evaluation export failed\n' >&2
      return 1
    fi
    return 0
  fi

  source_archive="$native_evidence_export_temporary/source-v1.tar.gz"
  if ! native_evidence_remote_stream_observation_archive \
    "$from_date" "$to_date" "$snapshot" >"$source_archive"; then
    printf 'error: read-only adjustment evaluation export failed\n' >&2
    return 1
  fi

  # bind the staged source archive into the selected exact v2 envelope
  if [[ $mode == daily_v2 ]]; then
    if ! (
      native_evidence_remote_exec_root stream-v2-envelope daily_monitoring \
        "$source_archive" "$from_date" "$to_date" "$snapshot"
    ); then
      printf 'error: adjustment monitoring envelope export failed\n' >&2
      return 1
    fi
  elif ! (
    native_evidence_remote_exec_root stream-v2-envelope post_burn_confirmation \
      "$source_archive" "$from_date" "$to_date" "$snapshot" \
      "$authorization_envelope"
  ); then
    printf 'error: adjustment confirmation envelope export failed\n' >&2
    return 1
  fi
)

# dispatch one forced-ssh operation
native_evidence_remote_dispatch() {
  local action=${1-}
  # reserve status 64 for caller fallback
  [[ -n "$action" ]] || return 64
  shift

  # require an absolute executable identity
  [[ ${native_evidence_binary-} == /* ]] || {
    printf 'error: native evidence binary is not an absolute path\n' >&2
    return 70
  }

  # preserve each installed operand grammar
  case "$action" in
    adjustment-family-release-status)
      # admit one action digest
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_exec_root family-release-status "$1"
      ;;
    adjustment-family-release-current-v1)
      # admit one closed family name
      if (($# != 1)) || [[ ! $1 =~ ^(temperature|wind|rain)$ ]]; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_exec_root family-release-current "$1"
      ;;
    adjustment-family-release-lineage-v2)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_root family-release-lineage-v2
      ;;
    adjustment-revision-capture-epoch-v1)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_root revision-capture-epoch-read-v1
      ;;
    adjustment-revision-capture-epoch-snapshot-v1)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_root revision-capture-epoch-snapshot-read-v1
      ;;
    adjustment-shadow-metadata-custody-status-v1)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_service shadow-metadata-custody-status-v1
      ;;
    adjustment-revision-custody-ack-v1|adjustment-revision-custody-ack-v2)
      # admit the complete custody identity
      if (($# != 10)) ||
        ! native_evidence_remote_is_bigint "$1" ||
        ! native_evidence_remote_is_sha256 "$2" ||
        ! native_evidence_remote_is_sha256 "$3" ||
        ! native_evidence_remote_is_bigint "$4" ||
        ! native_evidence_remote_is_sha256 "$5" ||
        ! native_evidence_remote_is_sha256 "$6" ||
        ! native_evidence_remote_is_sha256 "$7" ||
        ! native_evidence_remote_is_sha256 "$8" ||
        ! native_evidence_remote_is_sha256 "$9" ||
        [[ ! ${10} =~ ^(none|[a-f0-9]{64})$ ]]; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_exec_service \
        "${action/adjustment-revision/revision-cold}" "$@"
      ;;
    adjustment-revision-gap-start-v1)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_service revision-gap-start-v1
      ;;
    adjustment-revision-gap-page-v1|adjustment-revision-gap-ack-v1)
      # admit two graph identities
      if (($# != 2)) || ! native_evidence_remote_is_sha256 "$1" ||
        ! native_evidence_remote_is_sha256 "$2"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_exec_service "${action#adjustment-}" "$1" "$2"
      ;;
    adjustment-maintenance-anchor-status-v2)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_root maintenance-anchor-status-v2
      ;;
    adjustment-development-custody-anchor-current-v1|adjustment-rain-control-custody-anchor-current-v1)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_root "${action#adjustment-}"
      ;;
    adjustment-development-custody-anchor-install-v1|adjustment-rain-control-custody-anchor-install-v1|adjustment-unsupported-terminal-proof-install-v1|adjustment-maintenance-anchor-install-v2|adjustment-maintenance-anchor-install-v3)
      # admit one canonical document digest
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_has_anchor_authority || {
        printf 'error: native evidence anchor authority is unavailable\n' >&2
        return 69
      }
      native_evidence_remote_exec_root "${action#adjustment-}" "$1"
      ;;
    adjustment-unsupported-terminal-proof-current-v1)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_root unsupported-terminal-proof-current-v1
      ;;
    adjustment-future-input-seal-current-v2|adjustment-maintenance-anchor-current-v3)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_has_anchor_authority || {
        printf 'error: native evidence anchor authority is unavailable\n' >&2
        return 69
      }
      native_evidence_remote_exec_root "${action#adjustment-}"
      ;;
    adjustment-future-input-seal-install-v2|adjustment-maintenance-anchor-finalize-v2|adjustment-maintenance-anchor-finalize-v3)
      # admit one canonical document digest
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_exec_root "${action#adjustment-}" "$1"
      ;;
    adjustment-archive-next)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_exec_service archive-next
      ;;
    adjustment-archive-ack|adjustment-archive-ack-final)
      # admit one archive identity pair
      if (($# != 2)) || ! native_evidence_remote_is_sha256 "$1" ||
        ! native_evidence_remote_is_sha256 "$2"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_exec_service "${action#adjustment-}" "$1" "$2"
      ;;
    adjustment-database-ledger-v3)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_run_database_ledger
      ;;
    adjustment-registration-schedule-status-v3)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_evidence_remote_run_registration_schedule
      ;;
    adjustment-revision-catalog-current-start-v1)
      # admit one canonical cutoff timestamp
      if (($# != 1)) ||
        [[ ! $1 =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z$ ]]; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_run_revision_start "$1" "" 1
      ;;
    adjustment-revision-catalog-start-v1)
      # admit one cutoff and signed-bigint watermark
      if (($# != 2)) ||
        [[ ! $1 =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z$ ]] ||
        ! native_evidence_remote_is_bigint "$2"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_run_revision_start "$1" "$2" 0
      ;;
    adjustment-revision-catalog-page-v1)
      # admit the complete direct-successor cursor
      if (($# != 6)) ||
        ! native_evidence_remote_is_bigint "$1" ||
        ! native_evidence_remote_is_sha256 "$2" ||
        ! native_evidence_remote_is_sha256 "$3" ||
        ! native_evidence_remote_is_bigint "$4" ||
        ! native_evidence_remote_is_sha256 "$5" ||
        ! native_evidence_remote_is_sha256 "$6"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_run_revision_page "$@"
      ;;
    adjustment-shadow-metadata-custody-finalize-v1)
      # admit one current custody proof
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_host_shadow_finalize "$1"
      ;;
    adjustment-registration-schedule-initialize-v3)
      # admit one schedule bootstrap identity
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_host_registration_initialize "$1"
      ;;
    adjustment-registration-lifecycle-status-v4)
      (($# == 0)) || { native_evidence_remote_invalid_arguments; return; }
      native_host_registration_lifecycle
      ;;
    adjustment-confirmation-access-burn-v3)
      # admit one confirmation access identity
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_host_owner_access "$1"
      ;;
    adjustment-confirmation-availability-v2)
      # admit one registration identity
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_run_confirmation_availability "$1"
      ;;
    forecast-training-export)
      # preserve the original closed inclusive training window
      if (($# != 2)) || ! validate_calendar_date_range "$1" "$2" 450; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_host_forecast_training_export "$1" "$2"
      ;;
    adjustment-evaluation-export|adjustment-evaluation-export-v2)
      # admit the original closed local-date window
      if (($# != 2)) || ! validate_calendar_date_range "$1" "$2" 14; then
        native_evidence_remote_invalid_arguments
        return
      fi
      # select only the closed daily v2 variant
      if [[ $action == adjustment-evaluation-export-v2 ]]; then
        native_evidence_remote_run_evaluation_export daily_v2 "$1" "$2"
      else
        native_evidence_remote_run_evaluation_export legacy_v1 "$1" "$2"
      fi
      ;;
    adjustment-confirmation-export-v2)
      # admit one post-burn authorization identity and chunk
      if (($# != 3)) ||
        ! native_evidence_remote_is_sha256 "$1" ||
        ! native_evidence_remote_is_sha256 "$2" ||
        [[ ! $3 =~ ^([0-9]|1[0-9]|2[0-6])$ ]]; then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_evidence_remote_run_evaluation_export \
        confirmation_v2 "" "" "$1" "$2" "$3"
      ;;
    adjustment-shadow-unsupported-terminal-record-v1|adjustment-shadow-unsupported-terminal-retire-v1)
      # admit exactly one retained request identity
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      # invoke only the closed unsupported owner kinds
      if [[ $action == adjustment-shadow-unsupported-terminal-record-v1 ]]; then
        native_host_owner_operation unsupported-terminal "$1"
      else
        native_host_owner_operation unsupported-retire "$1"
      fi
      ;;
    adjustment-shadow-terminal-record-v3|adjustment-shadow-terminal-retire-v3)
      # select the qualified owner only for one exact request identity
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      if [[ $action == adjustment-shadow-terminal-record-v3 ]]; then
        native_host_owner_operation terminal "$1"
      else
        native_host_owner_operation retire "$1"
      fi
      ;;
    install-adjustment-scorecard|install-adjustment-scorecard-v2)
      # retain the original exact mode and addressed immutable document
      if (($# != 1)) || ! native_evidence_remote_is_sha256 "$1"; then
        native_evidence_remote_invalid_arguments
        return
      fi
      if [[ $action == install-adjustment-scorecard-v2 ]]; then
        native_host_scorecard_install --v2 "$1"
      else
        native_host_scorecard_install "$1"
      fi
      ;;
    adjustment-family-release)
      # preserve the exact original nine-operand transaction grammar
      if (($# != 9)); then
        native_evidence_remote_invalid_arguments
        return
      fi
      native_host_family_release "$@"
      ;;
    *)
      return 64
      ;;
  esac
}
