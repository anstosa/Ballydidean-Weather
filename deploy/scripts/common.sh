#!/usr/bin/env bash
set -euo pipefail

deploy_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
repo_root=$(cd "$deploy_dir/.." && pwd)
compose_file="$deploy_dir/compose.yaml"
: "$repo_root"

# print an operator error
die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

# require an executable
require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

# require a regular file
require_file() {
  [[ -f "$1" ]] || die "required file not found: $1"
}

# validate immutable release tags
validate_release() {
  local release=$1
  local release_date
  [[ "$release" =~ ^([0-9]{4})\.([0-9]{2})\.([0-9]{2})-([1-9][0-9]?)$ ]] ||
    die "release must use YYYY.MM.DD-N"
  release_date="${BASH_REMATCH[1]}-${BASH_REMATCH[2]}-${BASH_REMATCH[3]}"
  [[ "$(date --date "$release_date" +%Y-%m-%d 2>/dev/null)" == "$release_date" ]] ||
    die "release contains an invalid date"
  [[ "$release" != latest && "$release" != dev ]] || die "release must be immutable"
}

# validate one canonical calendar date
validate_calendar_date() {
  local value=$1
  [[ "$value" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] ||
    die "date must use YYYY-MM-DD"
  [[ "$(date -u --date "$value" +%F 2>/dev/null)" == "$value" ]] ||
    die "date contains an invalid calendar day"
}

# validate one bounded inclusive date range
validate_calendar_date_range() {
  local from_date=$1
  local to_date=$2
  local max_days=$3
  local from_epoch
  local to_epoch
  local inclusive_days
  [[ "$max_days" =~ ^[1-9][0-9]*$ ]] || die "maximum days must be positive"
  validate_calendar_date "$from_date"
  validate_calendar_date "$to_date"
  from_epoch=$(date -u --date "$from_date" +%s)
  to_epoch=$(date -u --date "$to_date" +%s)
  inclusive_days=$(( (to_epoch - from_epoch) / 86400 + 1 ))

  # reject reversed and oversized windows
  if ((inclusive_days < 1 || inclusive_days > max_days)); then
    die "export range must contain 1 to $max_days inclusive dates"
  fi
}

# require one digest-pinned image reference
validate_image_reference() {
  local image=$1
  [[ "$image" =~ ^[^@[:space:]]+@sha256:[a-f0-9]{64}$ ]] ||
    die "image must be a complete name@sha256 digest reference"
}

# read one validated release marker
read_release_state() {
  local path=$1
  local mode
  local -a lines
  require_file "$path"
  [[ ! -L "$path" ]] || die "release state must not be a symbolic link: $path"
  mode=$(stat -c '%a' "$path")
  (( (8#$mode & 077) == 0 )) || die "release state must be private: $path"
  mapfile -t lines <"$path"
  ((${#lines[@]} == 1)) || die "release state must contain exactly one line: $path"
  validate_release "${lines[0]}"
  printf '%s\n' "${lines[0]}"
}

# read absent state as empty
read_optional_release_state() {
  local path=$1

  # distinguish absence from unsafe links
  if [[ ! -e "$path" && ! -L "$path" ]]; then
    return 0
  fi

  read_release_state "$path"
}

# select active or bootstrap configuration
default_env_file() {
  local current
  current=$(read_optional_release_state "$deploy_dir/state/current-release")

  # derive active configuration from committed state
  if [[ -n "$current" ]]; then
    local active="$deploy_dir/releases/$current.env"
    require_file "$active"
    printf '%s\n' "$active"
  else
    printf '%s\n' "$deploy_dir/.env"
  fi
}

# run the Weather-scoped Compose project
compose() {
  local env_file=${WEATHER_ENV_FILE:-$(default_env_file)}
  local project_name=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
  require_file "$env_file"
  docker compose --project-name "$project_name" --env-file "$env_file" -f "$compose_file" "$@"
}

# hold the shared Weather lifecycle lock before reading active release state
acquire_weather_lifecycle_lock() {
  local state_root="$deploy_dir/state"
  require_command flock
  [[ -d "$state_root" && ! -L "$state_root" &&
    "$(realpath -e "$state_root")" == "$state_root" &&
    "$(stat -c '%u' "$state_root")" == "$EUID" ]] ||
    die "Weather lifecycle lock root is missing, linked or foreign-owned"
  exec {weather_lifecycle_lock_fd}<"$state_root"
  flock --exclusive --nonblock "$weather_lifecycle_lock_fd" ||
    die "another Weather lifecycle operation is in flight"
  [[ "$(stat -Lc '%d:%i' "/proc/self/fd/$weather_lifecycle_lock_fd")" == \
    "$(stat -c '%d:%i' "$state_root")" ]] ||
    die "Weather lifecycle lock identity changed"
}

# read a simple env value without evaluating it
env_value() {
  local file=$1
  local name=$2
  local value
  local -a matches
  mapfile -t matches < <(grep -E "^${name}=" "$file" || true)
  ((${#matches[@]} == 1)) || die "expected exactly one $name in $file"
  value=${matches[0]#*=}
  [[ -n "$value" ]] || die "missing $name in $file"
  printf '%s\n' "$value"
}

# validate one PostgreSQL identifier
validate_database_name() {
  local database_name=$1
  [[ "$database_name" =~ ^[a-z][a-z0-9_]{0,62}$ ]] || die "invalid database name"
}

# require one canonical owned descendant
require_canonical_descendant() {
  local path=$1
  local root=$2
  local description=$3
  local canonical_path canonical_root
  require_command realpath
  canonical_path=$(realpath -m -- "$path")
  canonical_root=$(realpath -m -- "$root")
  [[ "$path" == "$canonical_path" ]] || die "$description must use a canonical path"
  [[ "$canonical_path" == "$canonical_root/"* ]] || die "$description escapes its Weather root"
  [[ ! -L "$path" ]] || die "$description must not be a symbolic link"
}

# hash the production deployment control plane
control_plane_digest() {
  local file
  local -a files
  mapfile -d '' -t files < <(
    find "$deploy_dir/scripts" "$deploy_dir/postgres" "$deploy_dir/systemd" \
      "$deploy_dir/sudoers" -type f -print0 | LC_ALL=C sort -z
  )
  files+=("$deploy_dir/compose.yaml")
  (
    cd "$repo_root"

    # hash stable relative paths and contents
    for file in "${files[@]}"; do
      printf '%s\0' "${file#"$repo_root/"}"
      sha256sum "$file" | awk '{print $1}'
    done
  ) | sha256sum | awk '{print $1}'
}

# evaluate one bounded canonical normal-release capacity document
normal_release_capacity_receipt() {
  local command=$1
  local input_path=$2
  local input_mode input_size

  # admit only the two reviewed formula domains
  if [[ "$command" != normal-stage-capacity &&
    "$command" != normal-activation-capacity ]]; then
    die "invalid normal release capacity command"
  fi
  require_file "$input_path"
  [[ ! -L "$input_path" ]] || die "normal release capacity input must not be a symbolic link"
  input_mode=$(stat -c '%a' "$input_path")
  input_size=$(stat -c '%s' "$input_path")

  # keep measured host facts owner-private and inside the fixed parser bound
  if (( (8#$input_mode & 077) != 0 || input_size < 2 || input_size > 16384 )); then
    die "normal release capacity input is invalid"
  fi
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" "$command" <"$input_path"
}

# apply the versioned runtime ACL contract
apply_runtime_database_acl() {
  local env_file=$1
  local database_name=$2
  validate_database_name "$database_name"
  WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      <"$deploy_dir/postgres/runtime-acl-v2.sql"
}

# verify the optional recurring annex without granting action authority
verify_recurring_maintenance_acl() {
  local env_file=$1
  local database_name=$2
  validate_database_name "$database_name"
  WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command "WITH
      expected_functions(name, identity, security_definer, return_type) AS (VALUES
        ('weather_append_adjustment_shadow_v3', 'public.weather_append_adjustment_shadow_v3(jsonb,text)', true, 'jsonb'),
        ('weather_guard_adjustment_shadow_terminal_result_v2', 'public.weather_guard_adjustment_shadow_terminal_result_v2()', false, 'trigger'),
        ('weather_record_adjustment_shadow_terminal_result_v2', 'public.weather_record_adjustment_shadow_terminal_result_v2(jsonb)', true, 'jsonb'),
        ('weather_retire_adjustment_shadow_registration_v2', 'public.weather_retire_adjustment_shadow_registration_v2(jsonb)', true, 'jsonb')
      ), resolved AS (
        SELECT expected.*, to_regprocedure(identity)::oid AS oid
        FROM expected_functions expected
      ), table_identity AS (
        SELECT to_regclass('public.adjustment_shadow_terminal_results_v2')::oid AS oid
      ), runtime_roles(name) AS (VALUES
        ('weather_api'), ('weather_ingest'), ('weather_training_export')
      ), present AS (
        SELECT count(*) AS count FROM pg_proc procedure
        JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
        WHERE namespace.nspname = 'public'
          AND procedure.proname IN (SELECT name FROM expected_functions)
      ), new_columns AS (
        SELECT count(*) AS count FROM pg_attribute attribute
        WHERE attribute.attrelid = to_regclass('public.adjustment_shadow_predictions_v2')
          AND attribute.attname IN ('candidate_sha256', 'source_sha256',
            'scored_min_valid_at', 'scored_max_valid_at')
          AND NOT attribute.attisdropped AND attribute.attnotnull
      )
      SELECT CASE WHEN present.count = 0 AND table_identity.oid IS NULL THEN
        new_columns.count = 0 AND NOT EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0019_adjustment_maintenance_recurring.sql')
      ELSE present.count = 4 AND new_columns.count = 4
        AND EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0019_adjustment_maintenance_recurring.sql')
        AND (SELECT count(*) FROM resolved WHERE oid IS NOT NULL) = 4
        AND EXISTS (SELECT 1 FROM pg_class relation
          WHERE relation.oid = table_identity.oid AND relation.relkind = 'r'
            AND relation.relowner = 'weather_owner'::regrole)
        AND NOT EXISTS (SELECT 1 FROM resolved expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          WHERE procedure.proowner <> 'weather_owner'::regrole
            OR procedure.provolatile <> 'v'
            OR procedure.prosecdef IS DISTINCT FROM expected.security_definer
            OR procedure.prorettype::regtype::text <> expected.return_type
            OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public'])
        AND (SELECT count(*) FROM pg_trigger trigger
          WHERE trigger.tgrelid = table_identity.oid AND NOT trigger.tgisinternal
            AND trigger.tgname IN ('adjustment_shadow_terminal_results_v2_guard_insert',
              'adjustment_shadow_terminal_results_v2_guard_mutation',
              'adjustment_shadow_terminal_results_v2_guard_truncate')
            AND trigger.tgenabled = 'O'
            AND trigger.tgfoid = to_regprocedure('public.weather_guard_adjustment_shadow_terminal_result_v2()')
            AND trigger.tgtype = CASE trigger.tgname
              WHEN 'adjustment_shadow_terminal_results_v2_guard_insert' THEN 7
              WHEN 'adjustment_shadow_terminal_results_v2_guard_mutation' THEN 27
              ELSE 34 END) = 3
        AND NOT EXISTS (SELECT 1 FROM resolved expected CROSS JOIN runtime_roles role
          WHERE has_function_privilege(role.name, expected.oid, 'EXECUTE'))
        AND NOT EXISTS (SELECT 1 FROM resolved expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          CROSS JOIN LATERAL aclexplode(coalesce(procedure.proacl,
            acldefault('f', procedure.proowner))) acl
          WHERE acl.grantee = 0 AND acl.privilege_type = 'EXECUTE')
        AND NOT EXISTS (SELECT 1 FROM table_identity identity CROSS JOIN runtime_roles role
          CROSS JOIN (VALUES ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'),
            ('TRUNCATE'), ('REFERENCES'), ('TRIGGER')) privilege(name)
          WHERE has_table_privilege(role.name, identity.oid, privilege.name))
        AND NOT EXISTS (SELECT 1 FROM table_identity identity
          JOIN pg_class relation ON relation.oid = identity.oid
          CROSS JOIN LATERAL aclexplode(coalesce(relation.relacl,
            acldefault('r', relation.relowner))) acl WHERE acl.grantee = 0)
        AND NOT EXISTS (SELECT 1 FROM table_identity identity
          JOIN pg_attribute attribute ON attribute.attrelid = identity.oid
          CROSS JOIN runtime_roles role
          CROSS JOIN (VALUES ('SELECT'), ('INSERT'), ('UPDATE'), ('REFERENCES')) privilege(name)
          WHERE attribute.attnum > 0 AND NOT attribute.attisdropped
            AND has_column_privilege(role.name, identity.oid, attribute.attnum, privilege.name))
      END FROM present CROSS JOIN table_identity CROSS JOIN new_columns"
}

# verify the optional server-assigned revision-frontier annex
verify_adjustment_revision_frontier_acl() {
  local env_file=$1
  local database_name=$2
  validate_database_name "$database_name"
  WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command "WITH
      expected_functions(name, identity, volatility, runtime_role) AS (VALUES
        ('weather_append_adjustment_shadow_v4',
          'public.weather_append_adjustment_shadow_v4(jsonb,text)', 'v', NULL::text),
        ('adjustment_revision_frontier_v1',
          'public.adjustment_revision_frontier_v1()', 's', 'weather_training_export'),
        ('adjustment_shadow_revision_admission_v1',
          'public.adjustment_shadow_revision_admission_v1(text,text,text,integer)',
          's', 'weather_api'),
        ('weather_bind_weather_record_revisions_v1',
          'public.weather_bind_weather_record_revisions_v1(jsonb)', 'v', 'weather_ingest'),
        ('weather_bind_forecast_anchor_revisions_v1',
          'public.weather_bind_forecast_anchor_revisions_v1(jsonb)', 'v', 'weather_ingest'),
        ('weather_bind_rain_gate_revision_v1',
          'public.weather_bind_rain_gate_revision_v1(jsonb)', 'v', 'weather_ingest'),
        ('weather_bind_ecmwf_temperature_revision_v1',
          'public.weather_bind_ecmwf_temperature_revision_v1(jsonb)', 'v', 'weather_ingest'),
        ('adjustment_revision_serving_snapshot_v1',
          'public.adjustment_revision_serving_snapshot_v1(timestamptz,bigint)',
          's', 'weather_training_export'),
        ('adjustment_weather_revision_admission_v1',
          'public.adjustment_weather_revision_admission_v1(jsonb)', 's', 'weather_api'),
        ('adjustment_forecast_anchor_revision_admission_v1',
          'public.adjustment_forecast_anchor_revision_admission_v1(jsonb)', 's', 'weather_api'),
        ('adjustment_rain_gate_revision_admission_v1',
          'public.adjustment_rain_gate_revision_admission_v1(jsonb)', 's', 'weather_api'),
        ('adjustment_ecmwf_temperature_revision_admission_v1',
          'public.adjustment_ecmwf_temperature_revision_admission_v1(jsonb)', 's', 'weather_api'),
        ('weather_mark_adjustment_revision_gap_v1',
          'public.weather_mark_adjustment_revision_gap_v1(text,jsonb,jsonb)',
          'v', 'weather_api,weather_ingest')
      ), resolved AS (
        SELECT expected.*, to_regprocedure(identity)::oid AS oid
        FROM expected_functions expected
      ), receipt_validator AS (
        SELECT to_regprocedure(
          'public.weather_adjustment_revision_receipt_valid_v1(jsonb)')::oid AS oid
      ), objects AS (
        SELECT to_regclass('public.adjustment_revision_frontier_v1')::oid AS frontier_oid,
          to_regclass('public.adjustment_revision_ordinal_v1')::oid AS sequence_oid
      ), runtime_roles(name) AS (VALUES
        ('weather_api'), ('weather_ingest'), ('weather_training_export')
      ), new_columns AS (
        SELECT count(*) AS count FROM (VALUES
          ('adjustment_shadow_predictions_v2', 'stage_receipt_sha256'),
          ('adjustment_shadow_predictions_v2', 'archive_commit_ordinal'),
          ('adjustment_shadow_predictions_v2', 'predecessor_frontier_sha256'),
          ('adjustment_shadow_predictions_v2', 'revision_frontier_sha256'),
          ('adjustment_shadow_predictions_v2', 'revision_receipt_sha256'),
          ('adjustment_shadow_predictions_v2', 'archive_committed_at'),
          ('weather_records', 'adjustment_revision_receipt'),
          ('forecast_anchor_records', 'adjustment_revision_receipt'),
          ('rain_adjustment_runs', 'adjustment_revision_receipt'),
          ('ecmwf_temperature_canary_runs', 'adjustment_revision_receipt')
        ) expected(relation_name, column_name)
        JOIN pg_attribute attribute
          ON attribute.attrelid = to_regclass('public.' || expected.relation_name)
          AND attribute.attname = expected.column_name AND NOT attribute.attisdropped
      ), present AS (
        SELECT count(*) AS count FROM pg_proc procedure
        JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
        WHERE namespace.nspname = 'public'
          AND procedure.proname IN (SELECT name FROM expected_functions)
      )
      SELECT CASE WHEN present.count = 0 AND objects.frontier_oid IS NULL
          AND objects.sequence_oid IS NULL THEN
        new_columns.count = 0 AND NOT EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0020_adjustment_revision_frontier.sql')
      ELSE present.count = 13 AND new_columns.count = 10
        AND EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0020_adjustment_revision_frontier.sql')
        AND (SELECT count(*) FROM resolved WHERE oid IS NOT NULL) = 13
        AND receipt_validator.oid IS NOT NULL
        AND EXISTS (SELECT 1 FROM pg_class relation
          WHERE relation.oid = objects.frontier_oid AND relation.relkind = 'r'
            AND relation.relowner = 'weather_owner'::regrole)
        AND EXISTS (SELECT 1 FROM pg_class relation
          WHERE relation.oid = objects.sequence_oid AND relation.relkind = 'S'
            AND relation.relowner = 'weather_owner'::regrole)
        AND NOT EXISTS (SELECT 1 FROM resolved expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          WHERE procedure.proowner <> 'weather_owner'::regrole
            OR procedure.provolatile <> expected.volatility::\"char\"
            OR procedure.prosecdef IS NOT true
            OR procedure.prorettype::regtype::text <> 'jsonb'
            OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public'])
        AND NOT EXISTS (SELECT 1 FROM resolved expected CROSS JOIN runtime_roles role
          WHERE has_function_privilege(role.name, expected.oid, 'EXECUTE')
            IS DISTINCT FROM (expected.runtime_role IS NOT NULL
              AND role.name = ANY(string_to_array(expected.runtime_role, ','))))
        AND EXISTS (SELECT 1 FROM pg_proc procedure
          WHERE procedure.oid = receipt_validator.oid
            AND procedure.proowner = 'weather_owner'::regrole
            AND procedure.provolatile = 'i'::\"char\"
            AND procedure.prosecdef IS false
            AND procedure.prorettype::regtype::text = 'boolean'
            AND procedure.proconfig = ARRAY['search_path=pg_catalog, public'])
        AND NOT EXISTS (SELECT 1 FROM runtime_roles role
          WHERE has_function_privilege(role.name, receipt_validator.oid, 'EXECUTE')
            IS DISTINCT FROM (role.name IN ('weather_api', 'weather_ingest')))
        AND NOT EXISTS (SELECT 1 FROM objects CROSS JOIN runtime_roles role
          CROSS JOIN (VALUES ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'),
            ('TRUNCATE'), ('REFERENCES'), ('TRIGGER')) privilege(name)
          WHERE has_table_privilege(role.name, objects.frontier_oid, privilege.name))
        AND NOT EXISTS (SELECT 1 FROM objects CROSS JOIN runtime_roles role
          CROSS JOIN (VALUES ('USAGE'), ('SELECT'), ('UPDATE')) privilege(name)
          WHERE has_sequence_privilege(role.name, objects.sequence_oid, privilege.name))
        AND NOT EXISTS (SELECT 1 FROM (VALUES
            ('weather_records'), ('forecast_anchor_records'),
            ('rain_adjustment_runs'), ('ecmwf_temperature_canary_runs')) relation(name)
          CROSS JOIN runtime_roles role
          WHERE has_column_privilege(role.name, 'public.' || relation.name,
            'adjustment_revision_receipt', 'SELECT,INSERT,UPDATE,REFERENCES'))
      END FROM present CROSS JOIN receipt_validator CROSS JOIN objects CROSS JOIN new_columns"
}

# verify the optional rolling-registration annex and aggregate metadata bound
verify_adjustment_rolling_registration_acl() {
  local env_file=$1
  local database_name=$2
  validate_database_name "$database_name"
  WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command "WITH
      expected_tables(name) AS (VALUES
        ('adjustment_registration_schedule_v3'),
        ('adjustment_registration_horizons_v3'),
        ('adjustment_shadow_registration_windows_v3')
      ), expected_functions(name, identity, return_type, volatility, security_definer,
          runtime_roles) AS (VALUES
        ('weather_guard_adjustment_registration_v3',
          'public.weather_guard_adjustment_registration_v3()', 'trigger', 'v', false,
          NULL::text),
        ('weather_reject_adjustment_registration_v3_mutation',
          'public.weather_reject_adjustment_registration_v3_mutation()', 'trigger', 'v', false,
          NULL::text),
        ('weather_initialize_adjustment_registration_schedule_v3',
          'public.weather_initialize_adjustment_registration_schedule_v3(jsonb)', 'jsonb', 'v', true,
          NULL::text),
        ('weather_register_adjustment_shadow_v3',
          'public.weather_register_adjustment_shadow_v3(jsonb)', 'jsonb', 'v', true,
          'weather_api,weather_ingest'),
        ('adjustment_shadow_registration_slot_v3',
          'public.adjustment_shadow_registration_slot_v3(text)', 'jsonb', 's', true,
          'weather_api,weather_ingest,weather_training_export')
      ), resolved_functions AS (
        SELECT expected.*, to_regprocedure(expected.identity)::oid AS oid
        FROM expected_functions expected
      ), expected_triggers(name, table_name, function_identity, trigger_type) AS (VALUES
        ('adjustment_registration_schedule_v3_guard_insert',
          'adjustment_registration_schedule_v3',
          'public.weather_guard_adjustment_registration_v3()', 7::smallint),
        ('adjustment_registration_schedule_v3_guard_mutation',
          'adjustment_registration_schedule_v3',
          'public.weather_reject_adjustment_registration_v3_mutation()', 27::smallint),
        ('adjustment_registration_schedule_v3_guard_truncate',
          'adjustment_registration_schedule_v3',
          'public.weather_reject_adjustment_registration_v3_mutation()', 34::smallint),
        ('adjustment_registration_horizons_v3_guard_insert',
          'adjustment_registration_horizons_v3',
          'public.weather_guard_adjustment_registration_v3()', 7::smallint),
        ('adjustment_registration_horizons_v3_guard_mutation',
          'adjustment_registration_horizons_v3',
          'public.weather_reject_adjustment_registration_v3_mutation()', 27::smallint),
        ('adjustment_registration_horizons_v3_guard_truncate',
          'adjustment_registration_horizons_v3',
          'public.weather_reject_adjustment_registration_v3_mutation()', 34::smallint),
        ('adjustment_shadow_registration_windows_v3_guard_insert',
          'adjustment_shadow_registration_windows_v3',
          'public.weather_guard_adjustment_registration_v3()', 7::smallint),
        ('adjustment_shadow_registration_windows_v3_guard_mutation',
          'adjustment_shadow_registration_windows_v3',
          'public.weather_reject_adjustment_registration_v3_mutation()', 27::smallint),
        ('adjustment_shadow_registration_windows_v3_guard_truncate',
          'adjustment_shadow_registration_windows_v3',
          'public.weather_reject_adjustment_registration_v3_mutation()', 34::smallint)
      ), runtime_roles(name) AS (VALUES
        ('weather_api'), ('weather_ingest'), ('weather_training_export')
      ), present AS (
        SELECT count(*)::integer AS object_count FROM (
          SELECT relation.oid FROM pg_class relation
          JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
          WHERE namespace.nspname = 'public'
            AND relation.relname IN (SELECT name FROM expected_tables)
          UNION ALL
          SELECT procedure.oid FROM pg_proc procedure
          JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
          WHERE namespace.nspname = 'public'
            AND procedure.proname IN (SELECT name FROM expected_functions)
        ) object
      ), metadata_size AS (
        SELECT coalesce(sum(pg_total_relation_size(relation.name::regclass)), 0) AS bytes
        FROM (VALUES
          ('adjustment_shadow_registrations_v2'),
          ('adjustment_shadow_predictions_v2'),
          ('adjustment_confirmation_accesses_v2'),
          ('adjustment_shadow_terminal_results_v2'),
          ('adjustment_revision_frontier_v1'),
          ('adjustment_registration_schedule_v3'),
          ('adjustment_registration_horizons_v3'),
          ('adjustment_shadow_registration_windows_v3')
        ) relation(name)
        WHERE to_regclass('public.' || relation.name) IS NOT NULL
      )
      SELECT CASE WHEN present.object_count = 0 THEN
        NOT EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0021_adjustment_rolling_registration.sql')
      ELSE present.object_count = 8
        AND EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0021_adjustment_rolling_registration.sql')
        AND metadata_size.bytes <= 1048576
        AND (SELECT count(*) FROM expected_tables expected
          JOIN pg_class relation ON relation.oid = to_regclass('public.' || expected.name)
          WHERE relation.relkind = 'r'
            AND relation.relowner = 'weather_owner'::regrole) = 3
        AND (SELECT count(*) FROM resolved_functions WHERE oid IS NOT NULL) = 5
        AND (SELECT count(*) FROM pg_proc procedure
          JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
          WHERE namespace.nspname = 'public'
            AND procedure.proname IN (SELECT name FROM expected_functions)) = 5
        AND NOT EXISTS (SELECT 1 FROM resolved_functions expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          WHERE procedure.proowner <> 'weather_owner'::regrole
            OR procedure.prorettype::regtype::text <> expected.return_type
            OR procedure.provolatile <> expected.volatility::\"char\"
            OR procedure.prosecdef IS DISTINCT FROM expected.security_definer
            OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public'])
        AND (SELECT count(*) FROM expected_triggers expected
          JOIN pg_class relation ON relation.relname = expected.table_name
          JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
            AND namespace.nspname = 'public'
          JOIN pg_trigger trigger ON trigger.tgrelid = relation.oid
            AND trigger.tgname = expected.name AND NOT trigger.tgisinternal
            AND trigger.tgenabled = 'O'
            AND trigger.tgfoid = to_regprocedure(expected.function_identity)::oid
            AND trigger.tgtype = expected.trigger_type) = 9
        AND (SELECT count(*) FROM pg_trigger trigger
          JOIN pg_class relation ON relation.oid = trigger.tgrelid
          JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
          WHERE namespace.nspname = 'public'
            AND relation.relname IN (SELECT name FROM expected_tables)
            AND NOT trigger.tgisinternal) = 9
        AND NOT EXISTS (SELECT 1 FROM expected_tables expected
          JOIN pg_class relation ON relation.oid = to_regclass('public.' || expected.name)
          CROSS JOIN runtime_roles role
          CROSS JOIN (VALUES ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'),
            ('TRUNCATE'), ('REFERENCES'), ('TRIGGER')) privilege(name)
          WHERE has_table_privilege(role.name, relation.oid, privilege.name))
        AND NOT EXISTS (SELECT 1 FROM expected_tables expected
          JOIN pg_class relation ON relation.oid = to_regclass('public.' || expected.name)
          JOIN pg_attribute attribute ON attribute.attrelid = relation.oid
            AND attribute.attnum > 0 AND NOT attribute.attisdropped
          CROSS JOIN runtime_roles role
          CROSS JOIN (VALUES ('SELECT'), ('INSERT'), ('UPDATE'), ('REFERENCES')) privilege(name)
          WHERE has_column_privilege(role.name, relation.oid, attribute.attnum, privilege.name))
        AND NOT EXISTS (SELECT 1 FROM resolved_functions expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          CROSS JOIN LATERAL aclexplode(coalesce(procedure.proacl,
            acldefault('f', procedure.proowner))) acl
          WHERE acl.grantee = 0 AND acl.privilege_type = 'EXECUTE')
        AND NOT EXISTS (SELECT 1 FROM resolved_functions expected
          CROSS JOIN runtime_roles role
          WHERE has_function_privilege(role.name, expected.oid, 'EXECUTE')
            IS DISTINCT FROM (expected.runtime_roles IS NOT NULL
              AND role.name = ANY(string_to_array(expected.runtime_roles, ','))))
      END FROM present CROSS JOIN metadata_size"
}

# verify the optional forecast product catalog without scanning its rows
verify_forecast_product_catalog_acl() {
  local env_file=$1
  local database_name=$2
  validate_database_name "$database_name"
  WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command "WITH
      expected_functions(name, identity) AS (VALUES
        ('weather_sync_forecast_products_after_insert',
          'public.weather_sync_forecast_products_after_insert()'),
        ('weather_sync_forecast_products_after_delete',
          'public.weather_sync_forecast_products_after_delete()'),
        ('weather_sync_forecast_products_after_truncate',
          'public.weather_sync_forecast_products_after_truncate()')
      ), resolved_functions AS (
        SELECT expected.*, to_regprocedure(expected.identity)::oid AS oid
        FROM expected_functions expected
      ), expected_triggers(name, function_identity, trigger_type) AS (VALUES
        ('weather_records_forecast_products_insert',
          'public.weather_sync_forecast_products_after_insert()', 4::smallint),
        ('weather_records_forecast_products_delete',
          'public.weather_sync_forecast_products_after_delete()', 8::smallint),
        ('weather_records_forecast_products_truncate',
          'public.weather_sync_forecast_products_after_truncate()', 32::smallint)
      ), objects AS (
        SELECT to_regclass('public.forecast_products')::oid AS catalog_oid,
          to_regclass('public.sources')::oid AS sources_oid,
          to_regclass('public.weather_records')::oid AS records_oid
      ), present AS (
        SELECT (catalog_oid IS NOT NULL)::integer +
          (SELECT count(*) FROM pg_proc procedure
            JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
            WHERE namespace.nspname = 'public'
              AND procedure.proname IN (SELECT name FROM expected_functions)) +
          (SELECT count(*) FROM pg_trigger trigger
            WHERE trigger.tgrelid = records_oid
              AND NOT trigger.tgisinternal
              AND trigger.tgname IN (SELECT name FROM expected_triggers)) AS object_count
        FROM objects
      ), runtime_roles(name) AS (VALUES
        ('weather_api'), ('weather_ingest'), ('weather_training_export')
      )
      SELECT CASE WHEN present.object_count = 0 THEN
        NOT EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0022_forecast_product_catalog.sql')
      ELSE present.object_count = 7
        AND EXISTS (SELECT 1 FROM schema_migrations
          WHERE name = '0022_forecast_product_catalog.sql'
            AND checksum = '8d04170868414cd17e78cde2a4d6dd8144dfd606cba4e171686523f045cb77d2')
        AND (SELECT relation.relkind = 'r'
            AND relation.relowner = 'weather_owner'::regrole
          FROM pg_class relation WHERE relation.oid = objects.catalog_oid)
        AND (SELECT count(*) FROM pg_attribute attribute
          WHERE attribute.attrelid = objects.catalog_oid
            AND attribute.attnum > 0 AND NOT attribute.attisdropped) = 2
        AND (SELECT count(*) FROM (VALUES
          ('source_id', 'bigint', 1),
          ('product_run_at', 'timestamp with time zone', 2)
        ) expected(name, type_name, ordinal)
          JOIN pg_attribute attribute ON attribute.attrelid = objects.catalog_oid
            AND attribute.attname = expected.name
            AND attribute.attnum = expected.ordinal
            AND attribute.attnotnull
            AND format_type(attribute.atttypid, attribute.atttypmod) = expected.type_name) = 2
        AND (SELECT count(*) FROM pg_constraint constraint_
          WHERE constraint_.conrelid = objects.catalog_oid
            AND constraint_.contype = 'p'
            AND constraint_.conkey = ARRAY[
              (SELECT attnum FROM pg_attribute WHERE attrelid = objects.catalog_oid
                AND attname = 'source_id'),
              (SELECT attnum FROM pg_attribute WHERE attrelid = objects.catalog_oid
                AND attname = 'product_run_at')
            ]::smallint[]) = 1
        AND (SELECT count(*) FROM pg_constraint constraint_
          WHERE constraint_.conrelid = objects.catalog_oid
            AND constraint_.confrelid = objects.sources_oid
            AND constraint_.contype = 'f'
            AND constraint_.confdeltype = 'r'
            AND constraint_.confupdtype = 'a'
            AND constraint_.conkey = ARRAY[
              (SELECT attnum FROM pg_attribute WHERE attrelid = objects.catalog_oid
                AND attname = 'source_id')
            ]::smallint[]
            AND constraint_.confkey = ARRAY[
              (SELECT attnum FROM pg_attribute WHERE attrelid = objects.sources_oid
                AND attname = 'id')
            ]::smallint[]) = 1
        AND (SELECT count(*) FROM resolved_functions WHERE oid IS NOT NULL) = 3
        AND NOT EXISTS (SELECT 1 FROM resolved_functions expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          WHERE procedure.proowner <> 'weather_owner'::regrole
            OR procedure.prorettype::regtype::text <> 'trigger'
            OR procedure.provolatile <> 'v'::\"char\"
            OR NOT procedure.prosecdef
            OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public'])
        AND (SELECT count(*) FROM expected_triggers expected
          JOIN pg_trigger trigger ON trigger.tgrelid = objects.records_oid
            AND trigger.tgname = expected.name
            AND NOT trigger.tgisinternal
            AND trigger.tgenabled = 'O'
            AND trigger.tgfoid = to_regprocedure(expected.function_identity)::oid
            AND trigger.tgtype = expected.trigger_type) = 3
        AND (SELECT count(*) FROM pg_trigger trigger
          WHERE trigger.tgrelid = objects.records_oid
            AND NOT trigger.tgisinternal
            AND trigger.tgname LIKE 'weather_records_forecast_products_%') = 3
        AND has_table_privilege('weather_api', objects.catalog_oid, 'SELECT')
        AND NOT has_table_privilege('weather_api', objects.catalog_oid,
          'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND NOT EXISTS (SELECT 1 FROM runtime_roles role
          CROSS JOIN (VALUES ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'),
            ('TRUNCATE'), ('REFERENCES'), ('TRIGGER')) privilege(name)
          WHERE role.name <> 'weather_api'
            AND has_table_privilege(role.name, objects.catalog_oid, privilege.name))
        AND NOT EXISTS (SELECT 1 FROM pg_class relation
          CROSS JOIN LATERAL aclexplode(coalesce(relation.relacl,
            acldefault('r', relation.relowner))) acl
          WHERE relation.oid = objects.catalog_oid
            AND (acl.grantee = 0
              OR (acl.grantee = 'weather_api'::regrole
                AND acl.privilege_type <> 'SELECT')
              OR acl.grantee IN (
                'weather_ingest'::regrole, 'weather_training_export'::regrole)))
        AND NOT EXISTS (SELECT 1 FROM pg_attribute attribute
          CROSS JOIN LATERAL aclexplode(attribute.attacl) acl
          WHERE attribute.attrelid = objects.catalog_oid
            AND attribute.attnum > 0 AND NOT attribute.attisdropped
            AND attribute.attacl IS NOT NULL
            AND acl.grantee IN (0, 'weather_api'::regrole,
              'weather_ingest'::regrole, 'weather_training_export'::regrole))
        AND NOT EXISTS (SELECT 1 FROM resolved_functions expected
          CROSS JOIN runtime_roles role
          WHERE has_function_privilege(role.name, expected.oid, 'EXECUTE'))
        AND NOT EXISTS (SELECT 1 FROM resolved_functions expected
          JOIN pg_proc procedure ON procedure.oid = expected.oid
          CROSS JOIN LATERAL aclexplode(coalesce(procedure.proacl,
            acldefault('f', procedure.proowner))) acl
          WHERE acl.grantee = 0 AND acl.privilege_type = 'EXECUTE')
      END FROM present CROSS JOIN objects"
}

# verify effective runtime grants and denials
verify_runtime_database_acl() {
  local env_file=$1
  local database_name=$2
  local verified maintenance_verified recurring_verified revision_frontier_verified
  local rolling_registration_verified forecast_product_catalog_verified
  validate_database_name "$database_name"
  verified=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command "SELECT
        has_database_privilege('weather_api', current_database(), 'CONNECT')
        AND NOT has_database_privilege('weather_api', current_database(), 'CREATE')
        AND NOT has_database_privilege('weather_api', current_database(), 'TEMP')
        AND has_schema_privilege('weather_api', 'public', 'USAGE')
        AND NOT has_schema_privilege('weather_api', 'public', 'CREATE')
        AND has_table_privilege('weather_api', 'sites', 'SELECT')
        AND NOT has_table_privilege('weather_api', 'sites', 'INSERT')
        AND has_column_privilege('weather_api', 'sources', 'source_key', 'SELECT')
        AND has_column_privilege('weather_api', 'sources', 'capabilities', 'SELECT')
        AND NOT has_column_privilege('weather_api', 'sources', 'material_provider_config', 'SELECT')
        AND has_database_privilege('weather_ingest', current_database(), 'CONNECT')
        AND NOT has_database_privilege('weather_ingest', current_database(), 'CREATE')
        AND NOT has_database_privilege('weather_ingest', current_database(), 'TEMP')
        AND NOT has_schema_privilege('weather_ingest', 'public', 'CREATE')
        AND has_table_privilege('weather_ingest', 'sources', 'SELECT')
        AND has_table_privilege('weather_ingest', 'schema_migrations', 'SELECT')
        AND NOT has_table_privilege('weather_ingest', 'schema_migrations', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND has_table_privilege('weather_ingest', 'ingestion_runs', 'INSERT')
        AND has_table_privilege('weather_ingest', 'ingestion_runs', 'UPDATE')
        AND NOT has_table_privilege('weather_ingest', 'ingestion_runs', 'DELETE')
        AND has_column_privilege('weather_ingest', 'weather_records', 'water_level_m', 'UPDATE')
        AND NOT has_table_privilege('weather_api', 'forecast_anchor_records', 'SELECT')
        AND NOT has_table_privilege('weather_api', 'forecast_anchor_records', 'INSERT')
        AND NOT has_table_privilege('weather_ingest', 'forecast_anchor_records', 'SELECT')
        AND ((to_regclass('public.adjustment_revision_frontier_v1') IS NULL
            AND has_table_privilege('weather_ingest', 'forecast_anchor_records', 'INSERT'))
          OR (to_regclass('public.adjustment_revision_frontier_v1') IS NOT NULL
            AND NOT has_table_privilege('weather_ingest', 'forecast_anchor_records', 'INSERT')
            AND has_column_privilege('weather_ingest', 'forecast_anchor_records',
              'source_id', 'INSERT')))
        AND NOT has_table_privilege('weather_ingest', 'forecast_anchor_records', 'DELETE')
        AND has_column_privilege('weather_ingest', 'forecast_anchor_records', 'content_hash', 'SELECT')
        AND has_column_privilege('weather_ingest', 'forecast_anchor_records', 'revision_count', 'SELECT')
        AND NOT has_column_privilege('weather_ingest', 'forecast_anchor_records', 'id', 'SELECT')
        AND NOT has_column_privilege('weather_ingest', 'forecast_anchor_records', 'source_id', 'UPDATE')
        AND has_column_privilege('weather_ingest', 'forecast_anchor_records', 'revision_count', 'UPDATE')
        AND has_sequence_privilege('weather_ingest', 'forecast_anchor_records_id_seq', 'USAGE')
        AND has_sequence_privilege('weather_ingest', 'weather_records_id_seq', 'USAGE')
        AND has_table_privilege('weather_api', 'rain_collection_status_v1', 'SELECT')
        AND NOT has_table_privilege('weather_api', 'rain_collection_status_v1', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND has_table_privilege('weather_ingest', 'rain_collection_status_v1', 'SELECT')
        AND NOT has_table_privilege('weather_ingest', 'rain_collection_status_v1', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND has_table_privilege('weather_ingest', 'rain_capture_claims', 'SELECT')
        AND has_table_privilege('weather_ingest', 'rain_capture_claims', 'INSERT')
        AND has_table_privilege('weather_ingest', 'rain_capture_receipts', 'SELECT')
        AND has_table_privilege('weather_ingest', 'rain_capture_receipts', 'INSERT')
        AND NOT has_table_privilege('weather_ingest', 'rain_capture_claims', 'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND NOT has_table_privilege('weather_ingest', 'rain_capture_receipts', 'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND NOT EXISTS (
          SELECT 1
          FROM unnest(ARRAY['rain_capture_claims', 'rain_capture_receipts']) relation(name)
          CROSS JOIN LATERAL unnest(ARRAY['weather_api', 'weather_training_export']) consumer(name)
          CROSS JOIN LATERAL unnest(ARRAY['SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', 'REFERENCES', 'TRIGGER']) privilege(name)
          WHERE has_table_privilege(consumer.name, relation.name, privilege.name)
        )
        AND NOT EXISTS (
          SELECT 1
          FROM unnest(ARRAY['rain_capture_claims', 'rain_capture_receipts']) relation(name)
          JOIN pg_class table_relation ON table_relation.oid = relation.name::regclass
          JOIN pg_attribute attribute ON attribute.attrelid = table_relation.oid
            AND attribute.attnum > 0 AND NOT attribute.attisdropped
          CROSS JOIN LATERAL unnest(ARRAY['weather_api', 'weather_training_export']) consumer(name)
          CROSS JOIN LATERAL unnest(ARRAY['SELECT', 'INSERT', 'UPDATE', 'REFERENCES']) privilege(name)
          WHERE has_column_privilege(consumer.name, table_relation.oid, attribute.attnum, privilege.name)
        )
        AND NOT EXISTS (
          SELECT 1
          FROM unnest(ARRAY['rain_capture_claims', 'rain_capture_receipts']) relation(name)
          JOIN pg_class table_relation ON table_relation.oid = relation.name::regclass
          JOIN pg_attribute attribute ON attribute.attrelid = table_relation.oid
            AND attribute.attnum > 0 AND NOT attribute.attisdropped
          WHERE has_column_privilege('weather_ingest', table_relation.oid, attribute.attnum, 'UPDATE')
        )
        AND NOT has_table_privilege('weather_training_export', 'rain_collection_status_v1', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND ((to_regclass('public.adjustment_revision_frontier_v1') IS NULL
            AND has_table_privilege('weather_api', 'rain_adjustment_runs', 'SELECT'))
          OR (to_regclass('public.adjustment_revision_frontier_v1') IS NOT NULL
            AND NOT has_table_privilege('weather_api', 'rain_adjustment_runs', 'SELECT')
            AND has_column_privilege('weather_api', 'rain_adjustment_runs',
              'run_initialized_at', 'SELECT')))
        AND NOT has_table_privilege('weather_api', 'rain_adjustment_runs', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND ((to_regclass('public.adjustment_revision_frontier_v1') IS NULL
            AND has_table_privilege('weather_ingest', 'rain_adjustment_runs', 'SELECT')
            AND has_table_privilege('weather_ingest', 'rain_adjustment_runs', 'INSERT'))
          OR (to_regclass('public.adjustment_revision_frontier_v1') IS NOT NULL
            AND NOT has_table_privilege('weather_ingest', 'rain_adjustment_runs', 'SELECT')
            AND NOT has_table_privilege('weather_ingest', 'rain_adjustment_runs', 'INSERT')
            AND has_column_privilege('weather_ingest', 'rain_adjustment_runs',
              'run_initialized_at', 'SELECT')
            AND has_column_privilege('weather_ingest', 'rain_adjustment_runs',
              'run_initialized_at', 'INSERT')))
        AND NOT has_table_privilege('weather_ingest', 'rain_adjustment_runs', 'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND NOT has_table_privilege('weather_training_export', 'rain_adjustment_runs', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        AND NOT EXISTS (
          SELECT 1
          FROM pg_attribute attribute
          CROSS JOIN LATERAL unnest(ARRAY['weather_api', 'weather_ingest', 'weather_training_export']) consumer(name)
          CROSS JOIN LATERAL unnest(ARRAY['SELECT', 'INSERT', 'UPDATE', 'REFERENCES']) privilege(name)
          WHERE attribute.attrelid = 'rain_adjustment_runs'::regclass
            AND attribute.attnum > 0 AND NOT attribute.attisdropped
            AND (consumer.name = 'weather_training_export'
              OR privilege.name IN ('UPDATE', 'REFERENCES')
              OR (consumer.name = 'weather_api' AND privilege.name = 'INSERT'))
            AND has_column_privilege(consumer.name, 'rain_adjustment_runs', attribute.attnum, privilege.name)
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_auth_members membership
          WHERE membership.member IN ('weather_api'::regrole, 'weather_ingest'::regrole)
        )
        AND has_database_privilege('weather_training_export', current_database(), 'CONNECT')
        AND NOT has_database_privilege('weather_training_export', current_database(), 'CREATE')
        AND NOT has_database_privilege('weather_training_export', current_database(), 'TEMP')
        AND has_schema_privilege('weather_training_export', 'public', 'USAGE')
        AND NOT has_schema_privilege('weather_training_export', 'public', 'CREATE')
        AND has_table_privilege('weather_training_export', 'forecast_training_export_rows_v1', 'SELECT')
        AND has_table_privilege('weather_training_export', 'forecast_training_export_manifest_v1', 'SELECT')
        AND (
          (
            to_regclass('public.adjustment_evaluation_export_rows_v1') IS NULL
            AND to_regclass('public.adjustment_evaluation_export_manifest_v1') IS NULL
          )
          OR (
            to_regclass('public.adjustment_evaluation_export_rows_v1') IS NOT NULL
            AND to_regclass('public.adjustment_evaluation_export_manifest_v1') IS NOT NULL
            AND has_table_privilege('weather_training_export', 'adjustment_evaluation_export_rows_v1', 'SELECT')
            AND has_table_privilege('weather_training_export', 'adjustment_evaluation_export_manifest_v1', 'SELECT')
            AND NOT has_table_privilege('weather_api', 'adjustment_evaluation_export_rows_v1', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
            AND NOT has_table_privilege('weather_api', 'adjustment_evaluation_export_manifest_v1', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
            AND NOT has_table_privilege('weather_ingest', 'adjustment_evaluation_export_rows_v1', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
            AND NOT has_table_privilege('weather_ingest', 'adjustment_evaluation_export_manifest_v1', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
          )
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_namespace namespace
          WHERE namespace.nspname <> 'information_schema'
            AND namespace.nspname NOT LIKE 'pg_%'
            AND has_schema_privilege(
              'weather_training_export', namespace.oid, 'CREATE'
            )
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_namespace namespace
          CROSS JOIN LATERAL aclexplode(coalesce(
            namespace.nspacl,
            acldefault('n', namespace.nspowner)
          )) schema_acl
          WHERE namespace.nspname <> 'information_schema'
            AND namespace.nspname NOT LIKE 'pg_%'
            AND schema_acl.grantee = 'weather_training_export'::regrole
            AND schema_acl.privilege_type IN ('CREATE', 'USAGE')
            AND NOT (
              namespace.nspname = 'public'
              AND schema_acl.privilege_type = 'USAGE'
            )
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_class relation
          JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
          CROSS JOIN LATERAL unnest(ARRAY[
            'SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', 'REFERENCES', 'TRIGGER'
          ]) privilege(name)
          WHERE namespace.nspname <> 'information_schema'
            AND namespace.nspname NOT LIKE 'pg_%'
            AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
            AND has_table_privilege(
              'weather_training_export', relation.oid, privilege.name
            )
            AND NOT (
              namespace.nspname = 'public'
              AND relation.relname IN (
                'forecast_training_export_rows_v1',
                'forecast_training_export_manifest_v1',
                'adjustment_evaluation_export_rows_v1',
                'adjustment_evaluation_export_manifest_v1'
              )
              AND privilege.name = 'SELECT'
            )
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_class sequence
          JOIN pg_namespace namespace ON namespace.oid = sequence.relnamespace
          CROSS JOIN LATERAL unnest(ARRAY['SELECT', 'UPDATE', 'USAGE']) privilege(name)
          WHERE namespace.nspname <> 'information_schema'
            AND namespace.nspname NOT LIKE 'pg_%'
            AND sequence.relkind = 'S'
            AND has_sequence_privilege(
              'weather_training_export', sequence.oid, privilege.name
            )
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_proc procedure
          JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
          WHERE namespace.nspname <> 'information_schema'
            AND namespace.nspname NOT LIKE 'pg_%'
            AND procedure.prosecdef
            AND procedure.oid NOT IN (
              SELECT allowed.identity
              FROM (VALUES
                (to_regprocedure('public.adjustment_confirmation_availability_v2(text)')::oid),
                (to_regprocedure('public.adjustment_confirmation_export_v2(text,text,smallint)')::oid),
                (to_regprocedure('public.adjustment_revision_frontier_v1()')::oid),
                (to_regprocedure(
                  'public.adjustment_revision_serving_snapshot_v1(timestamptz,bigint)'
                )::oid),
                (to_regprocedure(
                  'public.adjustment_shadow_registration_slot_v3(text)'
                )::oid)
              ) allowed(identity)
              WHERE allowed.identity IS NOT NULL
            )
            AND has_function_privilege(
              'weather_training_export', procedure.oid, 'EXECUTE'
            )
        )
        AND EXISTS (
          SELECT 1
          FROM pg_roles
          WHERE rolname = 'weather_training_export'
            AND rolcanlogin
            AND NOT rolinherit
            AND NOT rolsuper
            AND NOT rolcreatedb
            AND NOT rolcreaterole
            AND NOT rolreplication
            AND NOT rolbypassrls
            AND rolconfig = ARRAY['default_transaction_read_only=on']
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_auth_members
          WHERE member = 'weather_training_export'::regrole
        )
        AND NOT EXISTS (
          SELECT 1
          FROM pg_db_role_setting
          WHERE setrole = 'weather_training_export'::regrole
            AND setdatabase <> 0
        )")
  maintenance_verified=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command "WITH
        expected_tables(name) AS (VALUES
          ('adjustment_shadow_registrations_v2'),
          ('adjustment_shadow_predictions_v2'),
          ('adjustment_confirmation_accesses_v2')
        ),
        expected_functions(name, identity, return_type, volatility, security_definer) AS (VALUES
          ('weather_guard_adjustment_shadow_registration_v2',
            'public.weather_guard_adjustment_shadow_registration_v2()', 'trigger', 'v', false),
          ('weather_guard_adjustment_shadow_prediction_v2',
            'public.weather_guard_adjustment_shadow_prediction_v2()', 'trigger', 'v', false),
          ('weather_guard_adjustment_confirmation_access_v2',
            'public.weather_guard_adjustment_confirmation_access_v2()', 'trigger', 'v', false),
          ('weather_reject_adjustment_maintenance_mutation',
            'public.weather_reject_adjustment_maintenance_mutation()', 'trigger', 'v', false),
          ('weather_register_adjustment_shadow_v2',
            'public.weather_register_adjustment_shadow_v2(jsonb)', 'jsonb', 'v', true),
          ('weather_append_adjustment_temperature_shadow_v2',
            'public.weather_append_adjustment_temperature_shadow_v2(jsonb)', 'jsonb', 'v', true),
          ('weather_append_adjustment_wind_shadow_v2',
            'public.weather_append_adjustment_wind_shadow_v2(jsonb)', 'jsonb', 'v', true),
          ('weather_append_adjustment_rain_shadow_v2',
            'public.weather_append_adjustment_rain_shadow_v2(jsonb)', 'jsonb', 'v', true),
          ('adjustment_shadow_body_admission_v2',
            'public.adjustment_shadow_body_admission_v2(text,text,text,integer)', 'boolean', 's', true),
          ('weather_finalize_adjustment_shadow_metadata_v2',
            'public.weather_finalize_adjustment_shadow_metadata_v2(jsonb)', 'jsonb', 'v', true),
          ('weather_record_adjustment_confirmation_access_v2',
            'public.weather_record_adjustment_confirmation_access_v2(jsonb)', 'jsonb', 'v', true),
          ('adjustment_confirmation_availability_v2',
            'public.adjustment_confirmation_availability_v2(text)', 'jsonb', 's', true),
          ('adjustment_confirmation_export_v2',
            'public.adjustment_confirmation_export_v2(text,text,smallint)', 'jsonb', 's', true)
        ),
        resolved_functions AS (
          SELECT expected.*,
            to_regprocedure(expected.identity)::oid AS oid
          FROM expected_functions expected
        ),
        expected_triggers(name, table_name, function_identity, trigger_type) AS (VALUES
          ('adjustment_shadow_registrations_v2_guard_insert',
            'adjustment_shadow_registrations_v2',
            'public.weather_guard_adjustment_shadow_registration_v2()', 7::smallint),
          ('adjustment_shadow_predictions_v2_guard_insert',
            'adjustment_shadow_predictions_v2',
            'public.weather_guard_adjustment_shadow_prediction_v2()', 7::smallint),
          ('adjustment_confirmation_accesses_v2_guard_insert',
            'adjustment_confirmation_accesses_v2',
            'public.weather_guard_adjustment_confirmation_access_v2()', 7::smallint),
          ('adjustment_shadow_registrations_v2_guard_mutation',
            'adjustment_shadow_registrations_v2',
            'public.weather_reject_adjustment_maintenance_mutation()', 27::smallint),
          ('adjustment_shadow_predictions_v2_guard_mutation',
            'adjustment_shadow_predictions_v2',
            'public.weather_reject_adjustment_maintenance_mutation()', 27::smallint),
          ('adjustment_confirmation_accesses_v2_guard_mutation',
            'adjustment_confirmation_accesses_v2',
            'public.weather_reject_adjustment_maintenance_mutation()', 27::smallint),
          ('adjustment_shadow_registrations_v2_guard_truncate',
            'adjustment_shadow_registrations_v2',
            'public.weather_reject_adjustment_maintenance_mutation()', 34::smallint),
          ('adjustment_shadow_predictions_v2_guard_truncate',
            'adjustment_shadow_predictions_v2',
            'public.weather_reject_adjustment_maintenance_mutation()', 34::smallint),
          ('adjustment_confirmation_accesses_v2_guard_truncate',
            'adjustment_confirmation_accesses_v2',
            'public.weather_reject_adjustment_maintenance_mutation()', 34::smallint)
        ),
        present AS (
          SELECT count(*)::integer AS object_count
          FROM (
            SELECT relation.oid
            FROM pg_class relation
            JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'public'
              AND relation.relname IN (SELECT name FROM expected_tables)
            UNION ALL
            SELECT procedure.oid
            FROM pg_proc procedure
            JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
            WHERE namespace.nspname = 'public'
              AND procedure.proname IN (SELECT name FROM expected_functions)
          ) object
        ),
        runtime_roles(name) AS (VALUES
          ('weather_api'), ('weather_ingest'), ('weather_training_export')
        )
      SELECT CASE
        WHEN present.object_count = 0 THEN true
        ELSE
          present.object_count = 16
          AND (SELECT count(*) FROM expected_tables table_expected
            JOIN pg_class relation ON relation.oid = to_regclass(
              'public.' || table_expected.name
            )
            WHERE relation.relkind = 'r'
              AND relation.relowner = 'weather_owner'::regrole) = 3
          AND (SELECT count(*) FROM resolved_functions WHERE oid IS NOT NULL) = 13
          AND (SELECT count(*)
            FROM pg_proc procedure
            JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
            WHERE namespace.nspname = 'public'
              AND procedure.proname IN (SELECT name FROM expected_functions)) = 13
          AND NOT EXISTS (
            SELECT 1
            FROM resolved_functions expected
            JOIN pg_proc procedure ON procedure.oid = expected.oid
            WHERE procedure.proowner <> 'weather_owner'::regrole
              OR procedure.prorettype::regtype::text <> expected.return_type
              OR procedure.provolatile <> expected.volatility
              OR procedure.prosecdef <> expected.security_definer
              OR procedure.proconfig IS DISTINCT FROM
                ARRAY['search_path=pg_catalog, public']::text[]
          )
          AND (SELECT count(*)
            FROM expected_triggers expected
            JOIN pg_class relation ON relation.relname = expected.table_name
            JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
              AND namespace.nspname = 'public'
            JOIN pg_trigger trigger ON trigger.tgrelid = relation.oid
              AND trigger.tgname = expected.name
              AND NOT trigger.tgisinternal
              AND trigger.tgfoid = to_regprocedure(expected.function_identity)::oid
              AND trigger.tgtype = expected.trigger_type) = 9
          AND (SELECT count(*)
            FROM pg_trigger trigger
            JOIN pg_class relation ON relation.oid = trigger.tgrelid
            JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = 'public'
              AND relation.relname IN (SELECT name FROM expected_tables)
              AND NOT trigger.tgisinternal) = 9
          AND NOT EXISTS (
            SELECT 1
            FROM expected_tables expected
            JOIN pg_class relation ON relation.oid = to_regclass(
              'public.' || expected.name
            )
            CROSS JOIN runtime_roles role
            CROSS JOIN LATERAL unnest(ARRAY[
              'SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', 'REFERENCES', 'TRIGGER'
            ]) privilege(name)
            WHERE has_table_privilege(role.name, relation.oid, privilege.name)
          )
          AND NOT EXISTS (
            SELECT 1
            FROM expected_tables expected
            JOIN pg_class relation ON relation.oid = to_regclass(
              'public.' || expected.name
            )
            JOIN pg_attribute attribute ON attribute.attrelid = relation.oid
              AND attribute.attnum > 0 AND NOT attribute.attisdropped
            CROSS JOIN runtime_roles role
            CROSS JOIN LATERAL unnest(ARRAY[
              'SELECT', 'INSERT', 'UPDATE', 'REFERENCES'
            ]) privilege(name)
            WHERE has_column_privilege(
              role.name, relation.oid, attribute.attnum, privilege.name
            )
          )
          AND NOT EXISTS (
            SELECT 1
            FROM resolved_functions expected
            JOIN pg_proc procedure ON procedure.oid = expected.oid
            CROSS JOIN LATERAL aclexplode(coalesce(
              procedure.proacl,
              acldefault('f', procedure.proowner)
            )) function_acl
            WHERE function_acl.grantee = 0
              AND function_acl.privilege_type = 'EXECUTE'
          )
          AND NOT EXISTS (
            SELECT 1
            FROM resolved_functions expected
            CROSS JOIN runtime_roles role
            WHERE has_function_privilege(role.name, expected.oid, 'EXECUTE')
              IS DISTINCT FROM CASE
                WHEN role.name = 'weather_api' THEN expected.name IN (
                  'weather_register_adjustment_shadow_v2',
                  'weather_append_adjustment_temperature_shadow_v2',
                  'weather_append_adjustment_wind_shadow_v2',
                  'adjustment_shadow_body_admission_v2'
                )
                WHEN role.name = 'weather_ingest' THEN expected.name IN (
                  'weather_register_adjustment_shadow_v2',
                  'weather_append_adjustment_rain_shadow_v2'
                )
                WHEN role.name = 'weather_training_export' THEN expected.name IN (
                  'adjustment_confirmation_availability_v2',
                  'adjustment_confirmation_export_v2'
                )
                ELSE false
              END
          )
          AND NOT EXISTS (
            SELECT 1
            FROM pg_db_role_setting setting
            CROSS JOIN LATERAL unnest(setting.setconfig) configuration(value)
            WHERE setting.setrole IN (
              'weather_owner'::regrole,
              'weather_api'::regrole,
              'weather_ingest'::regrole,
              'weather_training_export'::regrole
            )
              AND configuration.value =
                'weather.adjustment_maintenance_v2_enabled=on'
          )
      END
      FROM present")
  recurring_verified=$(verify_recurring_maintenance_acl "$env_file" "$database_name")
  revision_frontier_verified=$(verify_adjustment_revision_frontier_acl \
    "$env_file" "$database_name")
  rolling_registration_verified=$(verify_adjustment_rolling_registration_acl \
    "$env_file" "$database_name")
  forecast_product_catalog_verified=$(verify_forecast_product_catalog_acl \
    "$env_file" "$database_name")
  [[ "$verified" == t && "$maintenance_verified" == t && "$recurring_verified" == t \
      && "$revision_frontier_verified" == t && "$rolling_registration_verified" == t \
      && "$forecast_product_catalog_verified" == t ]] \
    || die "runtime database ACL verification failed"
}

# publish private state atomically
write_private_state() {
  (
  local path=$1
  local value=$2
  local temporary
  mkdir -p "$(dirname "$path")"
  umask 077
  temporary=$(mktemp "${path}.XXXXXX")

  # remove interrupted state writes
  trap 'rm -f "$temporary"' EXIT
  printf '%s\n' "$value" >"$temporary"
  chmod 600 "$temporary"
  sync -f "$temporary"
  mv "$temporary" "$path"
  sync -f "$(dirname "$path")"
  trap - EXIT
  )
}

# publish one release symlink atomically
write_active_symlink() {
  (
  local release=$1
  local path="$deploy_dir/state/active.env"
  local temporary_directory
  validate_release "$release"
  mkdir -p "$(dirname "$path")"
  temporary_directory=$(mktemp -d "$deploy_dir/state/.active.env.XXXXXX")

  # remove interrupted link writes
  trap 'rm -rf "$temporary_directory"' EXIT
  ln -s "../releases/$release.env" "$temporary_directory/active.env"
  mv -Tf "$temporary_directory/active.env" "$path"
  sync -f "$(dirname "$path")"
  rmdir "$temporary_directory"
  sync -f "$(dirname "$path")"
  trap - EXIT
  )
}
