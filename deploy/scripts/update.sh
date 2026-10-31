#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# print release usage
usage() {
  cat <<'EOF'
Usage:
  update.sh v14-compatibility-bridge RELEASE
  update.sh historical-v14-18-21-rehearsal RELEASE
  update.sh dashboard-release TARGET_RELEASE EXPECTED_CORE_RELEASE EXPECTED_WEB_SOURCE_RELEASE
  update.sh dashboard-rollback
  update.sh dashboard-rollback-core
  update.sh stage RELEASE
  update.sh activate RELEASE
  update.sh rollback
  update.sh recover
  update.sh adjustment-family-release TARGET_RELEASE COMPENSATING_RELEASE EXPECTED_CURRENT_RELEASE EXPECTED_SOURCE_RELEASE EXPECTED_SETTINGS_SHA256 FAMILY ACTION_SHA256 REPORT_SHA256 FENCE
  update.sh status

v14-compatibility-bridge performs only the fixed source-compatible image handoff
on the unchanged reviewed 0018 ledger before the full v14 transition.
historical-v14-18-21-rehearsal proves the exact atomic 0018-to-0021 migration
on an invocation-owned full-size clone without changing the active database.
stage resolves and persists four exact ARM64 image digests without changing
running services or the active database. Upgrade staging uses only a disposable
compatibility database. activate backs up before migration and records success
last. rollback changes images only and never invokes migration.
EOF
}

releases_dir="$deploy_dir/releases"
state_dir="$deploy_dir/state"
capacity_evidence=/var/lib/weather/preflight-latest.json
v13_resource_evidence=/var/lib/weather/preflight-v13-resource.json
control_plane_version=15
recurring_previous_release=2026.10.09-1
recurring_previous_control_plane_sha256=603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba
v14_compatibility_bridge_release=2026.10.09-2
v14_compatibility_bridge_commit=f6daa89d135661eb60dd5479c5d0d6c23e7198a9
full_v14_release=2026.10.09-3
# remain unavailable until the immutable bridge images are published
full_v14_source_server_image=ghcr.io/anstosa/weather-server@sha256:587dd0d34ed19086986a8081f7889fb10af2146d0d3f9ba46914389a787e5437
full_v14_source_web_image=ghcr.io/anstosa/weather-web@sha256:34d824b03c5f84289b8f38028e751f183a953565fc929260c09b07c9e64f15ba
v14_compatibility_bridge_state="$state_dir/v14-compatibility-bridge-v1"
v14_compatibility_bridge_pending="$state_dir/.v14-compatibility-bridge-v1.pending"
historical_v14_rehearsal_receipt="$state_dir/historical-v14-18-21-rehearsal-v1.json"
historical_v14_rehearsal_samples="$state_dir/historical-v14-18-21-rehearsal-samples-v1.json"
normal_activation_journal="$state_dir/normal-activation-v1.env"
maintenance_previous_release=2026.10.07-3
legacy_control_plane_version=6
legacy_control_plane_sha256=c4d74581b84505e065fdec63447dfdded1d14221e459777a88e37729275f33b5
migration_authorization_version=1
image_pull_required_free_bytes=2034155520
image_pull_required_free_inodes=32768
image_cleanup_maximum_inventory_lines=4096
image_cleanup_maximum_containers=1024
weather_server_image_repository=ghcr.io/anstosa/weather-server
weather_web_image_repository=ghcr.io/anstosa/weather-web
adjustment_family_state_root=/opt/weather/current/deploy/state/adjustment-release-state
adjustment_family_deploy_state=/opt/weather/current/deploy/state
adjustment_family_releases=/opt/weather/current/deploy/releases
adjustment_family_settings=/var/lib/weather/xweather/forecast-adjustment-settings.json
dashboard_release_journal="$state_dir/dashboard-web-transaction.env"
dashboard_release_journal_version=1

# locate one validated release environment
release_env() {
  validate_release "$1"
  printf '%s/%s.env\n' "$releases_dir" "$1"
}

# locate one isolated dashboard environment
dashboard_release_env() {
  validate_release "$1"
  printf '%s/%s.web.env\n' "$releases_dir" "$1"
}

# detect any active or interrupted dashboard override
dashboard_override_present() {
  [[ -e "$state_dir/current-web-release" || -L "$state_dir/current-web-release" ||
    -e "$state_dir/previous-web-release" || -L "$state_dir/previous-web-release" ||
    -e "$state_dir/active-web.env" || -L "$state_dir/active-web.env" ||
    -e "$dashboard_release_journal" || -L "$dashboard_release_journal" ]]
}

# require the installed root-owned dashboard state boundaries
require_dashboard_fixed_roots() {
  local path
  [[ "$deploy_dir" == /opt/weather/current/deploy &&
    "$state_dir" == /opt/weather/current/deploy/state &&
    "$releases_dir" == /opt/weather/current/deploy/releases ]] ||
    die "dashboard release roots differ from the installed contract"

  # reject aliases, foreign owners and mutable public roots
  for path in "$deploy_dir" "$state_dir" "$releases_dir"; do
    [[ -d "$path" && ! -L "$path" && "$(realpath -e "$path")" == "$path" &&
      "$(stat -c '%u:%g:%a' "$path")" == 0:0:775 ]] ||
      die "dashboard release root metadata is unsafe: $path"
  done
}

# keep unified releases behind the dashboard rollback boundary
require_no_dashboard_override() {
  if dashboard_override_present; then
    die "dashboard web override is active or interrupted; recover and rollback it first"
  fi
}

# hold one lifecycle lock on the fixed private state directory inode
acquire_release_transaction_lock() {
  require_command flock
  [[ -d "$state_dir" && ! -L "$state_dir" &&
    "$(realpath -e "$state_dir")" == "$state_dir" &&
    "$(stat -c '%u' "$state_dir")" == "$EUID" ]] ||
    die "release state lock root is missing, linked or foreign-owned"
  exec {release_transaction_lock_fd}<"$state_dir"
  flock --exclusive --nonblock "$release_transaction_lock_fd" ||
    die "another Weather release transaction is in flight"
  [[ "$(stat -Lc '%d:%i' "/proc/self/fd/$release_transaction_lock_fd")" == \
    "$(stat -c '%d:%i' "$state_dir")" ]] ||
    die "release state lock identity changed"
}

# derive v13 source settings only from the exact active predecessor
retained_maintenance_source_env() {
  local current source
  current=$(read_optional_release_state "$state_dir/current-release")
  [[ "$current" == "$maintenance_previous_release" ]] ||
    die "v13 deployment requires its exact retained active predecessor"
  source=$(release_env "$current")
  require_file "$source"
  printf '%s\n' "$source"
}

# refuse a bootstrap or caller-selected substitute for retained source settings
require_retained_maintenance_source() {
  [[ "$1" == "$(retained_maintenance_source_env)" ]] ||
    die "v13 deployment source must be the exact active release environment"
}

# select only the exact active v15 core and schema environment
normal_release_source_env() {
  local current schema source expected_link installed_digest
  current=$(read_release_state "$state_dir/current-release")
  schema=$(read_release_state "$state_dir/schema-release")
  [[ "$current" == "$schema" ]] ||
    die "normal release requires matching active core and schema releases"
  expected_link="../releases/$current.env"
  [[ -L "$state_dir/active.env" &&
    "$(readlink "$state_dir/active.env")" == "$expected_link" ]] ||
    die "normal release active environment differs from committed state"
  source=$(release_env "$current")
  validate_release_env "$source" "$current"
  require_control_plane_compatibility "$source"
  installed_digest=$(control_plane_digest)
  [[ "$(env_value "$source" WEATHER_CONTROL_PLANE_VERSION)" == "$control_plane_version" &&
    "$(env_value "$source" WEATHER_CONTROL_PLANE_SHA256)" == "$installed_digest" ]] ||
    die "normal release source is not the exact installed v15 contract"
  printf '%s\n' "$source"
}

# locate one schema-release authorization
migration_authorization() {
  validate_release "$1"
  printf '%s/%s.migration-authorization\n' "$releases_dir" "$1"
}

# validate one exact private authorization
validate_migration_authorization() {
  local path=$1
  local expected_runtime=$2
  local expected_schema=$3
  local mode version runtime_release schema_release history_sha256
  local meaningful_lines allowed_fields
  require_canonical_descendant "$path" "$releases_dir" "migration authorization"
  require_file "$path"
  mode=$(stat -c '%a' "$path")
  [[ "$mode" == 600 ]] || die "migration authorization must be private"
  meaningful_lines=$(grep -cE '^[A-Z][A-Z0-9_]*=' "$path")
  [[ "$meaningful_lines" -eq 4 ]] ||
    die "migration authorization must contain the exact current format"
  allowed_fields='^(WEATHER_MIGRATION_AUTHORIZATION_VERSION|WEATHER_MIGRATION_AUTHORIZATION_RELEASE|WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE|WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)='
  grep -qEv "$allowed_fields" "$path" &&
    die "migration authorization contains an unknown or malformed value"
  version=$(env_value "$path" WEATHER_MIGRATION_AUTHORIZATION_VERSION)
  runtime_release=$(env_value "$path" WEATHER_MIGRATION_AUTHORIZATION_RELEASE)
  schema_release=$(env_value "$path" WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE)
  history_sha256=$(env_value "$path" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)
  validate_release "$runtime_release"
  validate_release "$schema_release"

  # bind the authorization to both releases
  if [[ "$runtime_release" != "$expected_runtime" || "$schema_release" != "$expected_schema" ]]; then
    die "migration authorization release mismatch"
  fi

  [[ "$version" == "$migration_authorization_version" ]] ||
    die "unsupported migration authorization version"
  [[ "$history_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "invalid migration authorization history digest"
}

# publish one immutable authorization payload
write_migration_authorization() {
  local path=$1
  local runtime_release=$2
  local schema_release=$3
  local history_sha256=$4
  validate_release "$runtime_release"
  validate_release "$schema_release"
  [[ "$history_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "invalid migration authorization history digest"
  require_canonical_descendant "$path" "$releases_dir" "migration authorization"
  umask 077
  printf '%s\n' \
    "WEATHER_MIGRATION_AUTHORIZATION_VERSION=$migration_authorization_version" \
    "WEATHER_MIGRATION_AUTHORIZATION_RELEASE=$runtime_release" \
    "WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE=$schema_release" \
    "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=$history_sha256" >"$path"
  chmod 600 "$path"
  validate_migration_authorization "$path" "$runtime_release" "$schema_release"
}

# publish one authorization without replacement
publish_migration_authorization() {
  local source=$1
  local target=$2
  local runtime_release schema_release
  require_canonical_descendant "$source" "$releases_dir" "migration authorization"
  require_canonical_descendant "$target" "$releases_dir" "migration authorization"
  require_file "$source"
  runtime_release=$(env_value "$source" WEATHER_MIGRATION_AUTHORIZATION_RELEASE)
  schema_release=$(env_value "$source" WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE)
  validate_migration_authorization "$source" "$runtime_release" "$schema_release"

  # reject preexisting or raced publication
  [[ ! -e "$target" && ! -L "$target" ]] ||
    die "migration authorization already exists"
  sync -f "$source"
  ln "$source" "$target" || die "migration authorization already exists or could not be published"
  sync -f "$releases_dir"
  rm -f "$source"
  sync -f "$releases_dir"
  validate_migration_authorization "$target" "$runtime_release" "$schema_release"
}

# hash one exact ordered migration ledger
migration_history_sha256() {
  local env_file=$1
  local database_name=$2
  validate_database_name "$database_name"
  WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --field-separator=: \
      --command "SELECT name, checksum FROM schema_migrations ORDER BY name" |
    sha256sum | awk '{print $1}'
}

# write one frozen source-data contract without reading application rows into memory
write_preexisting_data_contract() {
  local env_file=$1
  local database_name=$2
  local output=$3
  local table all_columns semantic_columns metadata_sha256 pk_details pk_columns pk_index
  local sequence sequence_structure_sha256 bootstrap_sequence table_count=0 sequence_count=0
  local table_output column_output sequence_output
  local -a tables sequences columns semantic
  validate_database_name "$database_name"
  [[ -f "$output" && ! -L "$output" ]] ||
    die "preexisting data contract output is invalid"
  printf 'preexisting-data/v1\n' >"$output"

  # freeze every source application table in deterministic relation order
  if ! table_output=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
      --dbname "$database_name" --tuples-only --no-align --command \
      "SELECT relation.relname FROM pg_class relation JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace WHERE namespace.nspname = 'public' AND relation.relkind IN ('r','p') AND relation.relname <> 'schema_migrations' ORDER BY relation.relname"); then
    die "preexisting data table discovery failed"
  fi
  tables=()
  # parse only a successfully completed producer result
  if [[ -n "$table_output" ]]; then
    mapfile -t tables <<<"$table_output"
  fi
  ((${#tables[@]} > 0 && ${#tables[@]} <= 512)) ||
    die "preexisting data contract table count is invalid"
  for table in "${tables[@]}"; do
    [[ "$table" =~ ^[a-z][a-z0-9_]*$ ]] ||
      die "preexisting data table name is invalid"
    if ! column_output=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
      psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
        --dbname "$database_name" --tuples-only --no-align --command \
        "SELECT attribute.attname FROM pg_attribute attribute WHERE attribute.attrelid = 'public.\"$table\"'::regclass AND attribute.attnum > 0 AND NOT attribute.attisdropped ORDER BY attribute.attnum"); then
      die "preexisting data column discovery failed: $table"
    fi
    columns=()
    # parse only a successfully completed producer result
    if [[ -n "$column_output" ]]; then
      mapfile -t columns <<<"$column_output"
    fi
    ((${#columns[@]} > 0 && ${#columns[@]} <= 256)) ||
      die "preexisting data table column count is invalid"
    # reject names that cannot be quoted by this fixed proof implementation
    for all_columns in "${columns[@]}"; do
      [[ "$all_columns" =~ ^[a-z][a-z0-9_]*$ ]] ||
        die "preexisting data column name is invalid"
    done
    all_columns=$(IFS=,; printf '%s' "${columns[*]}")
    semantic=("${columns[@]}")
    case "$table" in
      sites|providers|stations|sources)
        # normalize only the bootstrap-owned volatile timestamp
        semantic=()
        for semantic_columns in "${columns[@]}"; do
          [[ "$semantic_columns" == updated_at ]] || semantic+=("$semantic_columns")
        done
        ((${#semantic[@]} + 1 == ${#columns[@]})) ||
          die "bootstrap configuration timestamp contract differs"
        ;;
    esac
    ((${#semantic[@]} > 0)) || die "preexisting data semantic column set is empty"
    semantic_columns=$(IFS=,; printf '%s' "${semantic[*]}")
    if ! metadata_sha256=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
      psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
        --dbname "$database_name" --tuples-only --no-align --field-separator='|' \
        --command "SELECT attribute.attnum, attribute.attname, format_type(attribute.atttypid, attribute.atttypmod), attribute.attnotnull, attribute.attidentity, attribute.attgenerated, COALESCE(pg_get_expr(default_value.adbin, default_value.adrelid), ''), COALESCE(collation_entry.collname, '') FROM pg_attribute attribute LEFT JOIN pg_attrdef default_value ON default_value.adrelid = attribute.attrelid AND default_value.adnum = attribute.attnum LEFT JOIN pg_collation collation_entry ON collation_entry.oid = attribute.attcollation WHERE attribute.attrelid = 'public.\"$table\"'::regclass AND attribute.attnum > 0 AND NOT attribute.attisdropped ORDER BY attribute.attnum" |
      sha256sum | awk '{print $1}'); then
      die "preexisting data column metadata failed: $table"
    fi
    if ! pk_details=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
      psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
        --dbname "$database_name" --tuples-only --no-align --field-separator=$'\t' \
        --command "SELECT string_agg(attribute.attname, ',' ORDER BY key.ordinality), index_relation.relname FROM pg_index primary_index JOIN pg_class index_relation ON index_relation.oid = primary_index.indexrelid CROSS JOIN LATERAL unnest(primary_index.indkey) WITH ORDINALITY key(attnum, ordinality) JOIN pg_attribute attribute ON attribute.attrelid = primary_index.indrelid AND attribute.attnum = key.attnum WHERE primary_index.indrelid = 'public.\"$table\"'::regclass AND primary_index.indisprimary AND key.ordinality <= primary_index.indnkeyatts GROUP BY index_relation.relname"); then
      die "preexisting data primary-key discovery failed: $table"
    fi
    IFS=$'\t' read -r pk_columns pk_index <<<"$pk_details"
    [[ "$metadata_sha256" =~ ^[a-f0-9]{64}$ &&
      "$pk_columns" =~ ^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$ &&
      "$pk_index" =~ ^[a-z][a-z0-9_]*$ ]] ||
      die "preexisting data table lacks one valid primary-key proof"
    printf 'T\t%s\t%s\t%s\t%s\t%s\t%s\n' "$table" "$all_columns" \
      "$semantic_columns" "$metadata_sha256" "$pk_columns" "$pk_index" >>"$output"
    table_count=$((table_count + 1))
  done

  # freeze every source sequence and its structural definition
  if ! sequence_output=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
      --dbname "$database_name" --tuples-only --no-align --command \
      "SELECT relation.relname FROM pg_class relation JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace WHERE namespace.nspname = 'public' AND relation.relkind = 'S' ORDER BY relation.relname"); then
    die "preexisting data sequence discovery failed"
  fi
  sequences=()
  # retain a legitimate empty sequence set only after producer success
  if [[ -n "$sequence_output" ]]; then
    mapfile -t sequences <<<"$sequence_output"
  fi
  ((${#sequences[@]} <= 512)) || die "preexisting data sequence count is invalid"
  for sequence in "${sequences[@]}"; do
    [[ "$sequence" =~ ^[a-z][a-z0-9_]*$ ]] ||
      die "preexisting data sequence name is invalid"
    if ! sequence_structure_sha256=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
      psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
        --dbname "$database_name" --tuples-only --no-align --field-separator='|' \
        --command "SELECT format_type(sequence.seqtypid, NULL), sequence.seqstart, sequence.seqincrement, sequence.seqmax, sequence.seqmin, sequence.seqcache, sequence.seqcycle FROM pg_sequence sequence WHERE sequence.seqrelid = 'public.\"$sequence\"'::regclass" |
      sha256sum | awk '{print $1}'); then
      die "preexisting data sequence metadata failed: $sequence"
    fi
    bootstrap_sequence=false
    case "$sequence" in
      sites_id_seq|providers_id_seq|stations_id_seq|sources_id_seq)
        bootstrap_sequence=true
        ;;
    esac
    [[ "$sequence_structure_sha256" =~ ^[a-f0-9]{64}$ ]] ||
      die "preexisting data sequence structure is invalid"
    printf 'S\t%s\t%s\t%s\n' "$sequence" "$sequence_structure_sha256" \
      "$bootstrap_sequence" >>"$output"
    sequence_count=$((sequence_count + 1))
  done
  ((table_count > 0 && table_count + sequence_count <= 1024)) ||
    die "preexisting data contract boundary is invalid"
  [[ $(stat -c '%s' "$output") -le 262144 ]] ||
    die "preexisting data contract exceeds its byte bound"
}

# write one bounded-memory ordered semantic snapshot for a frozen source contract
write_preexisting_data_snapshot() {
  local env_file=$1
  local database_name=$2
  local contract=$3
  local output=$4
  local kind name all_columns semantic_columns expected_metadata pk_columns pk_index extra
  local actual_metadata actual_pk_details actual_pk_columns actual_pk_index select_columns order_columns
  local plan row_proof sequence_structure last_value is_called bootstrap_sequence line_count=0
  local sequence_state
  validate_database_name "$database_name"
  [[ -f "$contract" && ! -L "$contract" && -f "$output" && ! -L "$output" &&
    $(stat -c '%s' "$contract") -le 262144 ]] ||
    die "preexisting data snapshot boundary is invalid"
  [[ $(head -n 1 "$contract") == preexisting-data/v1 ]] ||
    die "preexisting data contract version is invalid"
  printf 'preexisting-data-snapshot/v1\n' >"$output"

  # validate and stream each frozen table by its primary-key order
  while IFS=$'\t' read -r kind name all_columns semantic_columns expected_metadata \
    pk_columns pk_index extra; do
    ((line_count += 1))
    ((line_count <= 1024)) || die "preexisting data contract line count is invalid"
    [[ "$kind" == T || "$kind" == S ]] || die "preexisting data contract record is invalid"
    [[ "$name" =~ ^[a-z][a-z0-9_]*$ ]] || die "preexisting data object name is invalid"
    if [[ "$kind" == T ]]; then
      [[ -z "$extra" && "$all_columns" =~ ^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$ &&
        "$semantic_columns" =~ ^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$ &&
        "$expected_metadata" =~ ^[a-f0-9]{64}$ &&
        "$pk_columns" =~ ^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$ &&
        "$pk_index" =~ ^[a-z][a-z0-9_]*$ ]] ||
        die "preexisting data table contract is malformed"
      if ! actual_metadata=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
        psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
          --dbname "$database_name" --tuples-only --no-align --field-separator='|' \
          --command "SELECT attribute.attnum, attribute.attname, format_type(attribute.atttypid, attribute.atttypmod), attribute.attnotnull, attribute.attidentity, attribute.attgenerated, COALESCE(pg_get_expr(default_value.adbin, default_value.adrelid), ''), COALESCE(collation_entry.collname, '') FROM pg_attribute attribute LEFT JOIN pg_attrdef default_value ON default_value.adrelid = attribute.attrelid AND default_value.adnum = attribute.attnum LEFT JOIN pg_collation collation_entry ON collation_entry.oid = attribute.attcollation WHERE attribute.attrelid = 'public.\"$name\"'::regclass AND attribute.attnum > 0 AND NOT attribute.attisdropped AND attribute.attname = ANY(string_to_array('$all_columns', ',')) ORDER BY attribute.attnum" |
        sha256sum | awk '{print $1}'); then
        die "preexisting data table metadata snapshot failed: $name"
      fi
      [[ "$actual_metadata" == "$expected_metadata" ]] ||
        die "preexisting data table columns changed: $name"
      if ! actual_pk_details=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
        psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
          --dbname "$database_name" --tuples-only --no-align --field-separator=$'\t' \
          --command "SELECT string_agg(attribute.attname, ',' ORDER BY key.ordinality), index_relation.relname FROM pg_index primary_index JOIN pg_class index_relation ON index_relation.oid = primary_index.indexrelid CROSS JOIN LATERAL unnest(primary_index.indkey) WITH ORDINALITY key(attnum, ordinality) JOIN pg_attribute attribute ON attribute.attrelid = primary_index.indrelid AND attribute.attnum = key.attnum WHERE primary_index.indrelid = 'public.\"$name\"'::regclass AND primary_index.indisprimary AND key.ordinality <= primary_index.indnkeyatts GROUP BY index_relation.relname"); then
        die "preexisting data primary-key snapshot failed: $name"
      fi
      IFS=$'\t' read -r actual_pk_columns actual_pk_index <<<"$actual_pk_details"
      [[ "$actual_pk_columns" == "$pk_columns" && "$actual_pk_index" == "$pk_index" ]] ||
        die "preexisting data primary key changed: $name"
      select_columns=$(printf '%s' "$semantic_columns" | sed 's/^/"/; s/$/"/; s/,/","/g')
      order_columns=$(printf '%s' "$pk_columns" | sed 's/^/"/; s/$/"/; s/,/","/g')
      if ! plan=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
        psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
          --dbname "$database_name" --tuples-only --no-align --command \
          "BEGIN READ ONLY; SET LOCAL enable_seqscan = off; EXPLAIN (COSTS OFF) SELECT jsonb_build_array($select_columns)::text FROM public.\"$name\" ORDER BY $order_columns; ROLLBACK"); then
        die "preexisting data table plan snapshot failed: $name"
      fi
      [[ ( "$plan" == *"Index Scan"* || "$plan" == *"Index Only Scan"* ) &&
        "$plan" != *"Seq Scan"* && "$plan" != *"Sort"* ]] ||
        die "preexisting data table lacks its bounded primary-key scan: $name: $plan"
      # hash the stream in Node without shell interpolation
      # shellcheck disable=SC2016
      if ! row_proof=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
        psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
          --dbname "$database_name" --command \
          "BEGIN READ ONLY; SET LOCAL enable_seqscan = off; COPY (SELECT jsonb_build_array($select_columns)::text FROM public.\"$name\" ORDER BY $order_columns) TO STDOUT; COMMIT" |
        node --input-type=module -e '
          import { createHash } from "node:crypto";
          const hash = createHash("sha256");
          let rows = 0;
          process.stdin.on("data", (chunk) => {
            hash.update(chunk);
            for (const byte of chunk) if (byte === 10) rows += 1;
          });
          process.stdin.on("end", () => process.stdout.write(`${rows}\t${hash.digest("hex")}\n`));
        '); then
        die "preexisting data table row stream failed: $name"
      fi
      [[ "$row_proof" =~ ^[0-9]+$'\t'[a-f0-9]{64}$ ]] ||
        die "preexisting data row stream is invalid: $name"
      printf 'T\t%s\t%s\n' "$name" "$row_proof" >>"$output"
    else
      sequence_structure=$all_columns
      bootstrap_sequence=$semantic_columns
      [[ -z "$expected_metadata" && -z "$pk_columns" && -z "$pk_index" && -z "$extra" &&
        "$sequence_structure" =~ ^[a-f0-9]{64}$ &&
        ( "$bootstrap_sequence" == true || "$bootstrap_sequence" == false ) ]] ||
        die "preexisting data sequence contract is malformed"
      if ! actual_metadata=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
        psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
          --dbname "$database_name" --tuples-only --no-align --field-separator='|' \
          --command "SELECT format_type(sequence.seqtypid, NULL), sequence.seqstart, sequence.seqincrement, sequence.seqmax, sequence.seqmin, sequence.seqcache, sequence.seqcycle FROM pg_sequence sequence WHERE sequence.seqrelid = 'public.\"$name\"'::regclass" |
        sha256sum | awk '{print $1}'); then
        die "preexisting data sequence metadata snapshot failed: $name"
      fi
      [[ "$actual_metadata" == "$sequence_structure" ]] ||
        die "preexisting data sequence structure changed: $name"
      if ! sequence_state=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
        psql --no-psqlrc --quiet --set=ON_ERROR_STOP=1 --username postgres \
          --dbname "$database_name" --tuples-only --no-align --field-separator=$'\t' \
          --command "SELECT last_value, is_called FROM public.\"$name\""); then
        die "preexisting data sequence state snapshot failed: $name"
      fi
      IFS=$'\t' read -r last_value is_called <<<"$sequence_state"
      [[ "$last_value" =~ ^-?[0-9]+$ && ( "$is_called" == t || "$is_called" == f ) ]] ||
        die "preexisting data sequence state is invalid: $name"
      printf 'S\t%s\t%s\t%s\t%s\t%s\n' "$name" "$sequence_structure" \
        "$bootstrap_sequence" "$last_value" "$is_called" >>"$output"
    fi
  done < <(tail -n +2 "$contract")
  ((line_count > 0)) || die "preexisting data contract is empty"
  [[ $(stat -c '%s' "$output") -le 262144 ]] ||
    die "preexisting data snapshot exceeds its byte bound"
}

# compare two source-bound snapshots with only reviewed bootstrap sequence drift
verify_preexisting_data_snapshots() {
  local contract=$1
  local before=$2
  local after=$3
  node --input-type=module - "$contract" "$before" "$after" <<'NODE'
import { readFileSync } from "node:fs";

const [contractPath, beforePath, afterPath] = process.argv.slice(2);
const readBoundedLines = (path, header) => {
  const bytes = readFileSync(path);
  if (bytes.length < 1 || bytes.length > 262_144) throw new Error("semantic proof exceeds its byte bound");
  const lines = bytes.toString("utf8").trimEnd().split("\n");
  if (lines[0] !== header || lines.length < 2 || lines.length > 1_025) {
    throw new Error("semantic proof boundary is invalid");
  }
  return lines.slice(1);
};
const contract = new Map();
for (const line of readBoundedLines(contractPath, "preexisting-data/v1")) {
  const fields = line.split("\t");
  const key = `${fields[0]}:${fields[1]}`;
  const validTable = fields[0] === "T" && fields.length === 7 &&
    /^[a-z][a-z0-9_]*$/u.test(fields[1]) &&
    /^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$/u.test(fields[2]) &&
    /^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$/u.test(fields[3]) &&
    /^[a-f0-9]{64}$/u.test(fields[4]) &&
    /^[a-z][a-z0-9_]*(,[a-z][a-z0-9_]*)*$/u.test(fields[5]) &&
    /^[a-z][a-z0-9_]*$/u.test(fields[6]);
  const validSequence = fields[0] === "S" && fields.length === 4 &&
    /^[a-z][a-z0-9_]*$/u.test(fields[1]) && /^[a-f0-9]{64}$/u.test(fields[2]) &&
    (fields[3] === "true" || fields[3] === "false");
  if ((!validTable && !validSequence) || contract.has(key)) {
    throw new Error("semantic contract contains a duplicate or invalid record");
  }
  contract.set(key, fields);
}
const parseSnapshot = (path) => {
  const records = new Map();
  for (const line of readBoundedLines(path, "preexisting-data-snapshot/v1")) {
    const fields = line.split("\t");
    const key = `${fields[0]}:${fields[1]}`;
    if (!contract.has(key) || records.has(key)) throw new Error("semantic snapshot record is invalid");
    if ((fields[0] === "T" && (fields.length !== 4 || !/^\d+$/u.test(fields[2]) || !/^[a-f0-9]{64}$/u.test(fields[3]))) ||
      (fields[0] === "S" && (fields.length !== 6 || fields[2] !== contract.get(key)[2] ||
        fields[3] !== contract.get(key)[3] || !/^-?\d+$/u.test(fields[4]) || !/^[tf]$/u.test(fields[5])))) {
      throw new Error("semantic snapshot value is invalid");
    }
    records.set(key, fields);
  }
  if (records.size !== contract.size) throw new Error("semantic snapshot is incomplete");
  return records;
};
const beforeRecords = parseSnapshot(beforePath);
const afterRecords = parseSnapshot(afterPath);
for (const [key, definition] of contract) {
  const left = beforeRecords.get(key);
  const right = afterRecords.get(key);
  if (definition[0] === "T" || definition[3] === "false") {
    if (left.join("\t") !== right.join("\t")) throw new Error(`preexisting application data changed: ${definition[1]}`);
    continue;
  }
  if (left[2] !== right[2] || left[3] !== right[3] || BigInt(right[4]) < BigInt(left[4]) ||
    (left[5] === "t" && right[5] !== "t")) {
    throw new Error(`bootstrap sequence did not advance monotonically: ${definition[1]}`);
  }
}
NODE
}

# retain one bounded private failure packet before invocation-owned cleanup
retain_release_failure_evidence() {
  local label=$1
  local failure_status=$2
  shift 2
  local temporary final source name size total_size=0
  local manifest
  case "$label" in
    normal-stage|historical-v14-rehearsal) ;;
    *) return 1 ;;
  esac
  [[ "$failure_status" =~ ^[1-9][0-9]{0,2}$ && -d "$state_dir" && ! -L "$state_dir" ]] ||
    return 1
  umask 077
  temporary=$(mktemp -d "$state_dir/.${label}-failure.XXXXXX") || return 1
  chmod 700 "$temporary" || {
    rm -rf -- "$temporary"
    return 1
  }
  manifest="$temporary/manifest"
  printf 'contractVersion=release-qualification-failure/v1\nlabel=%s\nstatus=%s\n' \
    "$label" "$failure_status" >"$manifest" || {
    rm -rf -- "$temporary"
    return 1
  }
  # copy only the caller's bounded regular evidence members
  for source in "$@"; do
    [[ -n "$source" && -f "$source" && ! -L "$source" ]] || continue
    size=$(stat -c '%s' "$source") || {
      rm -rf -- "$temporary"
      return 1
    }
    [[ "$size" =~ ^[0-9]+$ && "$size" -le 1048576 ]] || {
      rm -rf -- "$temporary"
      return 1
    }
    total_size=$((total_size + size))
    ((total_size <= 4194304)) || {
      rm -rf -- "$temporary"
      return 1
    }
    name=$(basename -- "$source")
    [[ "$name" =~ ^[.a-zA-Z0-9_-]+$ && ! -e "$temporary/$name" ]] || {
      rm -rf -- "$temporary"
      return 1
    }
    install -m 600 -- "$source" "$temporary/$name" || {
      rm -rf -- "$temporary"
      return 1
    }
    printf 'member=%s:%s:%s\n' "$name" "$size" \
      "$(sha256sum "$temporary/$name" | awk '{print $1}')" >>"$manifest" || {
      rm -rf -- "$temporary"
      return 1
    }
  done
  chmod 600 "$manifest" || {
    rm -rf -- "$temporary"
    return 1
  }
  # publish only after every member and digest is complete
  for source in "$temporary"/*; do
    sync -f "$source" || {
      rm -rf -- "$temporary"
      return 1
    }
  done
  sync -f "$temporary" || {
    rm -rf -- "$temporary"
    return 1
  }
  final="$state_dir/${label}-failure-${temporary##*.}"
  [[ ! -e "$final" && ! -L "$final" ]] || {
    rm -rf -- "$temporary"
    return 1
  }
  mv -- "$temporary" "$final" || {
    rm -rf -- "$temporary"
    return 1
  }
  sync -f "$state_dir" || return 1
  printf 'Retained release qualification failure evidence: %s\n' "$final" >&2
}

# remove a mutable tag or digest
image_repository() {
  local image=$1
  local repository=${image%%@*}
  local leaf=${repository##*/}

  # strip only a tag after the final slash
  if [[ "$leaf" == *:* ]]; then
    repository=${repository%:*}
  fi

  [[ "$repository" =~ ^[a-z0-9][a-z0-9._/:=-]*$ ]] ||
    die "invalid image repository: $image"
  printf '%s\n' "$repository"
}

# require the protected floor and one complete next-capture reservation
require_image_pull_capacity_floor() {
  local available_blocks block_bytes free_inodes free_bytes
  require_command stat
  read -r available_blocks block_bytes free_inodes < <(
    stat --file-system --format='%a %S %d' /var/lib/weather
  )
  [[ "$available_blocks" =~ ^[0-9]+$ && "$block_bytes" =~ ^[1-9][0-9]*$ &&
    "$free_inodes" =~ ^[0-9]+$ ]] || die "image pull capacity sample is invalid"
  free_bytes=$((available_blocks * block_bytes))

  # refuse pulls without claiming the separate full release oracle passed
  if ((free_bytes < image_pull_required_free_bytes ||
    free_inodes < image_pull_required_free_inodes)); then
    die "image pull capacity is below the protected capture floor"
  fi
}

# enforce the fixed inert v13 literal source/target/compensation ledger
require_literal_adjustment_release_capacity() (
  local source_env=$1
  local target_env=$2
  local source_release target_version receipt status key
  local source_server source_web target_server target_web
  require_file "$source_env"
  require_file "$target_env"
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  target_version=$(env_value "$target_env" WEATHER_CONTROL_PLANE_VERSION)

  # this gate is intentionally limited to the reviewed v12-to-v13 inert bridge
  [[ "$source_release" == "$maintenance_previous_release" &&
    "$target_version" == 13 ]] ||
    die "literal adjustment release capacity is unsupported outside the fixed inert v13 bridge"

  # keep the direct bridge on the exact retained database and tunnel infrastructure
  for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR; do
    [[ "$(env_value "$source_env" "$key")" == "$(env_value "$target_env" "$key")" ]] ||
      die "fixed inert v13 infrastructure identity differs: $key"
  done
  source_server=$(env_value "$source_env" WEATHER_SERVER_IMAGE)
  source_web=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  target_server=$(env_value "$target_env" WEATHER_SERVER_IMAGE)
  target_web=$(env_value "$target_env" WEATHER_WEB_IMAGE)
  receipt=$(mktemp "${TMPDIR:-/tmp}/weather-release-capacity.XXXXXX.json") ||
    die "literal adjustment release receipt could not be created"
  trap 'rm -f -- "$receipt"' EXIT
  chmod 600 "$receipt" || die "literal adjustment release receipt is not private"
  status=0
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" release-capacity \
    "$source_server" "$source_web" "$target_server" "$target_web" \
    >"$receipt" || status=$?
  require_bounded_image_cleanup_snapshot "$receipt" 1

  # surface the closed refusal receipt without treating it as admission
  if ((status != 0)); then
    cat "$receipt" >&2
    die "literal adjustment release image capacity is blocked"
  fi
  if ! node --input-type=module - "$receipt" <<'NODE'
import { readFileSync } from "node:fs";

const receipt = JSON.parse(readFileSync(process.argv[2], "utf8"));

// require the exact successful no-retirement receipt
if (receipt.contractVersion !== "adjustment-release-capacity/v3" ||
  receipt.compensationScope !== "fixed-inert-v13-whole-release-source-restore" ||
  receipt.sourceRelease !== "2026.10.07-3" ||
  receipt.state !== "capacity_ready" || receipt.retirementCreditBytes !== 0 ||
  receipt.compatibilityFixtureBytes !== 64 * 1_024 * 1_024 ||
  receipt.compatibilityFixtureInodes !== 4_096 ||
  receipt.futureStateBytes !== 1 * 1_024 * 1_024 ||
  !Number.isSafeInteger(receipt.retainedControlBytes) ||
  receipt.retainedControlBytes < 0 ||
  !Array.isArray(receipt.imageDigests) || receipt.imageDigests.length !== 6 ||
  !Number.isSafeInteger(receipt.requiredFreeBytes) ||
  !Number.isSafeInteger(receipt.requiredFreeInodes)) {
  throw new Error("literal release capacity receipt is invalid");
}
NODE
  then
    die "literal adjustment release capacity receipt is invalid"
  fi
  printf 'Literal adjustment release capacity passed: %s\n' \
    "$(tr -d '\n' <"$receipt")" >&2
)

# enforce the fixed inert v14 literal source/target/compensation ledger
require_literal_inert_v14_release_capacity() (
  local source_env=$1
  local target_env=$2
  local source_release target_version receipt status key
  local source_server source_web target_server target_web
  require_file "$source_env"
  require_file "$target_env"
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  target_version=$(env_value "$target_env" WEATHER_CONTROL_PLANE_VERSION)

  # this gate is intentionally limited to the reviewed inactive v13-to-v14 inert bridge
  [[ "$source_release" == "$recurring_previous_release" &&
    "$target_version" == 15 ]] ||
    die "literal adjustment release capacity is unsupported outside the fixed inert v14 bridge"

  # keep the direct bridge on the exact retained database and tunnel infrastructure
  for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR; do
    [[ "$(env_value "$source_env" "$key")" == "$(env_value "$target_env" "$key")" ]] ||
      die "fixed inert v14 infrastructure identity differs: $key"
  done
  source_server=$(env_value "$source_env" WEATHER_SERVER_IMAGE)
  source_web=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  target_server=$(env_value "$target_env" WEATHER_SERVER_IMAGE)
  target_web=$(env_value "$target_env" WEATHER_WEB_IMAGE)
  receipt=$(mktemp "${TMPDIR:-/tmp}/weather-release-capacity.XXXXXX.json") ||
    die "literal adjustment release receipt could not be created"
  trap 'rm -f -- "$receipt"' EXIT
  chmod 600 "$receipt" || die "literal adjustment release receipt is not private"
  status=0
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" inert-v14-release-capacity \
    "$source_server" "$source_web" "$target_server" "$target_web" \
    >"$receipt" || status=$?
  require_bounded_image_cleanup_snapshot "$receipt" 1

  # surface the closed refusal receipt without treating it as admission
  if ((status != 0)); then
    cat "$receipt" >&2
    die "literal adjustment release image capacity is blocked"
  fi
  if ! node --input-type=module - "$receipt" <<'NODE'
import { readFileSync } from "node:fs";

const receipt = JSON.parse(readFileSync(process.argv[2], "utf8"));

// require the exact successful no-retirement receipt
if (receipt.contractVersion !== "adjustment-inert-v14-release-capacity/v1" ||
  receipt.compensationScope !== "fixed-inert-v14-whole-release-source-restore" ||
  receipt.sourceRelease !== "2026.10.09-1" ||
  receipt.state !== "capacity_ready" || receipt.retirementCreditBytes !== 0 ||
  receipt.compatibilityFixtureBytes !== 64 * 1_024 * 1_024 ||
  receipt.compatibilityFixtureInodes !== 4_096 ||
  receipt.futureStateBytes !== 1 * 1_024 * 1_024 ||
  !Number.isSafeInteger(receipt.retainedControlBytes) ||
  receipt.retainedControlBytes < 0 ||
  !Array.isArray(receipt.imageDigests) || receipt.imageDigests.length !== 6 ||
  !Number.isSafeInteger(receipt.requiredFreeBytes) ||
  !Number.isSafeInteger(receipt.requiredFreeInodes)) {
  throw new Error("literal release capacity receipt is invalid");
}
NODE
  then
    die "literal adjustment release capacity receipt is invalid"
  fi
  printf 'Literal adjustment release capacity passed: %s\n' \
    "$(tr -d '\n' <"$receipt")" >&2
)

# enforce the full v14 literal bridge/target/compensation ledger
require_literal_full_v14_release_capacity() (
  local source_env=$1
  local target_env=$2
  local source_release target_version receipt status key
  local source_server source_web target_server target_web
  require_file "$source_env"
  require_file "$target_env"
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  target_version=$(env_value "$target_env" WEATHER_CONTROL_PLANE_VERSION)

  # admit only the reviewed bridge-to-full transition
  [[ "$source_release" == "$v14_compatibility_bridge_release" &&
    "$target_version" == 15 ]] ||
    die "literal full v14 capacity is unsupported outside the fixed bridge handoff"

  # retain the exact database and tunnel infrastructure across the handoff
  for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR; do
    [[ "$(env_value "$source_env" "$key")" == "$(env_value "$target_env" "$key")" ]] ||
      die "full v14 infrastructure identity differs: $key"
  done
  source_server=$(env_value "$source_env" WEATHER_SERVER_IMAGE)
  source_web=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  target_server=$(env_value "$target_env" WEATHER_SERVER_IMAGE)
  target_web=$(env_value "$target_env" WEATHER_WEB_IMAGE)
  receipt=$(mktemp "${TMPDIR:-/tmp}/weather-full-v14-capacity.XXXXXX.json") ||
    die "full v14 capacity receipt could not be created"
  trap 'rm -f -- "$receipt"' EXIT
  chmod 600 "$receipt" || die "full v14 capacity receipt is not private"
  status=0
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" full-v14-release-capacity \
    "$source_server" "$source_web" "$target_server" "$target_web" \
    >"$receipt" || status=$?
  require_bounded_image_cleanup_snapshot "$receipt" 1

  # retain the complete refusal receipt without treating it as admission
  if ((status != 0)); then
    cat "$receipt" >&2
    die "literal full v14 capacity is blocked"
  fi
  if ! node --input-type=module - "$receipt" <<'NODE'
import { readFileSync } from "node:fs";

const receipt = JSON.parse(readFileSync(process.argv[2], "utf8"));

// require the exact successful bridge-restoration receipt
if (receipt.contractVersion !== "adjustment-full-v14-release-capacity/v1" ||
  receipt.compensationScope !== "fixed-full-v14-whole-release-source-restore" ||
  receipt.sourceRelease !== "2026.10.09-2" ||
  receipt.state !== "capacity_ready" || receipt.retirementCreditBytes !== 0 ||
  receipt.compatibilityFixtureBytes !== 64 * 1_024 * 1_024 ||
  receipt.compatibilityFixtureInodes !== 4_096 ||
  receipt.futureStateBytes !== 1 * 1_024 * 1_024 ||
  !Number.isSafeInteger(receipt.retainedControlBytes) ||
  receipt.retainedControlBytes < 0 ||
  !Array.isArray(receipt.imageDigests) || receipt.imageDigests.length !== 6 ||
  !Number.isSafeInteger(receipt.requiredFreeBytes) ||
  !Number.isSafeInteger(receipt.requiredFreeInodes)) {
  throw new Error("literal full v14 capacity receipt is invalid");
}
NODE
  then
    die "literal full v14 capacity receipt is invalid"
  fi
  printf 'Literal full v14 capacity passed: %s\n' \
    "$(tr -d '\n' <"$receipt")" >&2
)

# cap one frozen Docker inventory before parsing or comparison
require_bounded_image_cleanup_snapshot() {
  local path=$1
  local maximum_lines=$2
  local byte_count line_count
  byte_count=$(wc -c <"$path")
  line_count=$(wc -l <"$path")
  [[ "$byte_count" =~ ^[0-9]+$ && "$line_count" =~ ^[0-9]+$ ]] ||
    die "Docker image cleanup snapshot is invalid"

  # reject unexpected daemon scale instead of scanning without a contract
  if ((byte_count > 1048576 || line_count > maximum_lines)); then
    die "Docker image cleanup snapshot exceeds its bound"
  fi
}

# freeze every local image reference without deleting or normalizing it
snapshot_docker_image_inventory() {
  local target=$1
  docker image ls --all --no-trunc --digests \
    --format '{{.ID}}\t{{.Repository}}\t{{.Tag}}\t{{.Digest}}' |
    LC_ALL=C sort -u >"$target"
  require_bounded_image_cleanup_snapshot \
    "$target" "$image_cleanup_maximum_inventory_lines"
}

# freeze every running or stopped container image identity
snapshot_docker_container_image_ids() {
  local target=$1
  local containers_path="${target}.containers"
  local -a containers
  docker ps --all --quiet --no-trunc | LC_ALL=C sort -u >"$containers_path"
  require_bounded_image_cleanup_snapshot \
    "$containers_path" "$image_cleanup_maximum_containers"
  mapfile -t containers <"$containers_path"
  : >"$target"

  # inspect only the bounded exact container set
  if ((${#containers[@]} > 0)); then
    docker container inspect --format '{{.Image}}' "${containers[@]}" |
      LC_ALL=C sort -u >"$target"
  fi
  rm -f -- "$containers_path"
  require_bounded_image_cleanup_snapshot \
    "$target" "$image_cleanup_maximum_containers"
}

# append one environment's closed image references to the cleanup contract
append_image_cleanup_environment() {
  local env_file=$1
  local require_local=$2
  local repositories_path=$3
  local references_path=$4
  local protected_ids_path=$5
  local key reference repository expected_repository image_id
  require_file "$env_file"

  # bind all four release images and only the two Weather-owned repositories
  for key in WEATHER_SERVER_IMAGE WEATHER_WEB_IMAGE POSTGRES_IMAGE CLOUDFLARED_IMAGE; do
    reference=$(env_value "$env_file" "$key")
    validate_image_reference "$reference"
    printf '%s\n' "$reference" >>"$references_path"

    # scope deletion authority to Weather application repositories
    if [[ "$key" == WEATHER_SERVER_IMAGE || "$key" == WEATHER_WEB_IMAGE ]]; then
      repository=$(image_repository "$reference")
      expected_repository=$weather_server_image_repository

      # select the fixed web repository only for the web image field
      if [[ "$key" == WEATHER_WEB_IMAGE ]]; then
        expected_repository=$weather_web_image_repository
      fi
      [[ "$repository" == "$expected_repository" ]] ||
        die "Weather image repository is outside cleanup authority: $repository"
      printf '%s\n' "$repository" >>"$repositories_path"
    fi

    # require every retained current or previous image to remain locally resolvable
    if [[ "$require_local" == true ]]; then
      image_id=$(docker image inspect --format '{{.Id}}' "$reference") ||
        die "retained release image is unavailable: $reference"
      [[ "$image_id" =~ ^sha256:[a-f0-9]{64}$ ]] ||
        die "retained release image identity is invalid"
      printf '%s\n' "$image_id" >>"$protected_ids_path"
    fi
  done
}

# freeze the release state and its exact image protection contract
write_weather_image_cleanup_contract() {
  local source_env=$1
  local target_env=$2
  local state_path=$3
  local repositories_path=$4
  local references_path=$5
  local protected_ids_path=$6
  local current previous current_web previous_web retained_env retained_web_env
  current=$(read_optional_release_state "$state_dir/current-release")
  previous=$(read_optional_release_state "$state_dir/previous-release")
  printf 'current=%s\nprevious=%s\n' "$current" "$previous" >"$state_path"
  : >"$repositories_path"
  : >"$references_path"
  : >"$protected_ids_path"
  append_image_cleanup_environment \
    "$source_env" false "$repositories_path" "$references_path" "$protected_ids_path"
  append_image_cleanup_environment \
    "$target_env" false "$repositories_path" "$references_path" "$protected_ids_path"

  # protect both retained rollback boundaries by reference and local image identity
  for retained_env in "$current" "$previous"; do
    if [[ -n "$retained_env" ]]; then
      local retained_path
      retained_path=$(release_env "$retained_env")
      validate_release_env "$retained_path" "$retained_env"
      append_image_cleanup_environment \
        "$retained_path" true "$repositories_path" "$references_path" "$protected_ids_path"
    fi
  done

  current_web=$(read_optional_release_state "$state_dir/current-web-release")
  previous_web=$(read_optional_release_state "$state_dir/previous-web-release")

  # protect both dashboard override rollback boundaries independently
  for retained_web_env in "$current_web" "$previous_web"; do
    if [[ -n "$retained_web_env" && "$retained_web_env" != "$current" &&
      "$retained_web_env" != "$previous" ]]; then
      local retained_web_path
      retained_web_path=$(dashboard_release_env "$retained_web_env")
      validate_dashboard_release_env "$retained_web_path" "$retained_web_env" "$current"
      append_image_cleanup_environment \
        "$retained_web_path" true "$repositories_path" "$references_path" "$protected_ids_path"
    fi
  done
  LC_ALL=C sort -u -o "$repositories_path" "$repositories_path"
  LC_ALL=C sort -u -o "$references_path" "$references_path"
  LC_ALL=C sort -u -o "$protected_ids_path" "$protected_ids_path"
  require_bounded_image_cleanup_snapshot "$repositories_path" 16
  require_bounded_image_cleanup_snapshot "$references_path" 32
  require_bounded_image_cleanup_snapshot "$protected_ids_path" 16
}

# select only unprotected Weather-owned image identities from frozen files
select_obsolete_weather_image_ids() {
  local inventory_path=$1
  local repositories_path=$2
  local references_path=$3
  local protected_ids_path=$4
  node --input-type=module - \
    "$inventory_path" "$repositories_path" "$references_path" "$protected_ids_path" <<'NODE'
import { readFileSync, statSync } from "node:fs";

const [inventoryPath, repositoriesPath, referencesPath, protectedIdsPath] =
  process.argv.slice(2);
const imageIdPattern = /^sha256:[a-f0-9]{64}$/u;
const repositoryPattern = /^[a-z0-9][a-z0-9._/:=-]*$/u;
const tagPattern = /^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$/u;
const digestPattern = /^sha256:[a-f0-9]{64}$/u;
const referencePattern =
  /^[a-z0-9][a-z0-9._/:=-]*(?:@sha256:[a-f0-9]{64}|:[A-Za-z0-9_][A-Za-z0-9_.-]{0,127})$/u;
const expectedRepositories = [
  "ghcr.io/anstosa/weather-server",
  "ghcr.io/anstosa/weather-web",
];

// read one small canonical newline-delimited contract
function readContractLines(path, label, maximumLines) {
  const details = statSync(path);

  // reject non-files and oversized selector inputs
  if (!details.isFile() || details.size > 1_048_576) {
    throw new Error(`${label} is invalid`);
  }
  const text = readFileSync(path, "utf8");

  // reject alternate framing and incomplete final records
  if (text.includes("\0") || text.includes("\r") ||
    (text.length > 0 && !text.endsWith("\n"))) {
    throw new Error(`${label} framing is invalid`);
  }
  const lines = text.length === 0 ? [] : text.slice(0, -1).split("\n");

  // reject empty, duplicate or unexpectedly large contracts
  if (lines.length > maximumLines || lines.some(
    // reject empty entries inside a nonempty contract
    (line) => line.length === 0,
  ) || new Set(lines).size !== lines.length) {
    throw new Error(`${label} entries are invalid`);
  }
  return lines;
}

const repositories = readContractLines(repositoriesPath, "Weather repositories", 16);
const protectedReferences = readContractLines(referencesPath, "protected references", 32);
const protectedIds = readContractLines(protectedIdsPath, "protected image IDs", 2048);
const inventory = readContractLines(inventoryPath, "image inventory", 4096);

// require every Weather repository to use the closed Docker grammar
if (JSON.stringify(repositories) !== JSON.stringify(expectedRepositories) ||
  repositories.some(
  // reject malformed repository authority
  (repository) => !repositoryPattern.test(repository),
)) {
  throw new Error("Weather repository contract is invalid");
}

// require every protected reference to remain exact and printable
if (protectedReferences.some(
  // reject mutable whitespace or unsupported reference shapes
  (reference) => !referencePattern.test(reference),
)) {
  throw new Error("protected image reference is invalid");
}

// require every protected image to use one full local identity
if (protectedIds.some(
  // reject shortened or non-content-addressed identities
  (imageId) => !imageIdPattern.test(imageId),
)) {
  throw new Error("protected image identity is invalid");
}

const repositorySet = new Set(repositories);
const referenceSet = new Set(protectedReferences);
const protectedIdSet = new Set(protectedIds);
const images = new Map();

// group every tag and digest sharing one local image identity
for (const line of inventory) {
  const fields = line.split("\t");

  // require the exact Docker inventory field contract
  if (fields.length !== 4) {
    throw new Error("image inventory record is invalid");
  }
  const [imageId, repository, tag, digest] = fields;

  // reject partial IDs and noncanonical repository metadata
  if (!imageIdPattern.test(imageId) ||
    (repository !== "<none>" && !repositoryPattern.test(repository)) ||
    (tag !== "<none>" && !tagPattern.test(tag)) ||
    (digest !== "<none>" && !digestPattern.test(digest))) {
    throw new Error("image inventory fields are invalid");
  }
  const state = images.get(imageId) ?? {
    foreignRepository: false,
    protected: protectedIdSet.has(imageId),
    weatherRepository: false,
  };

  // classify only repository-bound rows; dangling rows grant no authority
  if (repository !== "<none>") {
    if (repositorySet.has(repository)) {
      state.weatherRepository = true;
    } else {
      state.foreignRepository = true;
    }

    // protect either exact tag or exact digest references
    if ((tag !== "<none>" && referenceSet.has(`${repository}:${tag}`)) ||
      (digest !== "<none>" && referenceSet.has(`${repository}@${digest}`))) {
      state.protected = true;
    }
  }
  images.set(imageId, state);
}

const selected = [];

// choose only exclusively Weather-owned, unprotected local identities
for (const [imageId, state] of images) {
  if (state.weatherRepository && !state.foreignRepository && !state.protected) {
    selected.push(imageId);
  }
}
selected.sort();

// emit one bounded canonical deletion plan
if (selected.length > 0) {
  process.stdout.write(`${selected.join("\n")}\n`);
}
NODE
}

# retain one candidate's exact current repository aliases
write_image_alias_snapshot() {
  local inventory_path=$1
  local image_id=$2
  local target=$3
  [[ "$image_id" =~ ^sha256:[a-f0-9]{64}$ ]] ||
    die "Docker image cleanup candidate is invalid"
  awk -F '\t' -v expected="$image_id" '$1 == expected' "$inventory_path" >"$target"
  require_bounded_image_cleanup_snapshot "$target" 64
}

# delete one frozen obsolete Weather image plan without force or pruning
cleanup_obsolete_weather_images() (
  local source_env=$1
  local target_env=$2
  local temporary candidates_path image_id
  local -a candidates
  require_command cmp
  require_command node
  temporary=$(mktemp -d "${TMPDIR:-/tmp}/weather-image-cleanup.XXXXXX")

  # remove only the private selector workspace
  trap 'rm -rf -- "$temporary"' EXIT
  snapshot_docker_image_inventory "$temporary/images.before"
  snapshot_docker_container_image_ids "$temporary/containers.before"
  write_weather_image_cleanup_contract \
    "$source_env" "$target_env" \
    "$temporary/state.before" "$temporary/repositories.before" \
    "$temporary/references.before" "$temporary/retained.before"
  cat "$temporary/containers.before" "$temporary/retained.before" |
    LC_ALL=C sort -u >"$temporary/protected.before"
  candidates_path="$temporary/candidates"
  select_obsolete_weather_image_ids \
    "$temporary/images.before" "$temporary/repositories.before" \
    "$temporary/references.before" "$temporary/protected.before" >"$candidates_path"
  mapfile -t candidates <"$candidates_path"

  # leave an already clean cache unchanged
  if ((${#candidates[@]} == 0)); then
    return
  fi

  # reauthorize and remove exactly one image at a time
  for image_id in "${candidates[@]}"; do
    write_image_alias_snapshot \
      "$temporary/images.before" "$image_id" "$temporary/aliases.before"
    snapshot_docker_image_inventory "$temporary/images.current"
    snapshot_docker_container_image_ids "$temporary/containers.current"
    write_weather_image_cleanup_contract \
      "$source_env" "$target_env" \
      "$temporary/state.current" "$temporary/repositories.current" \
      "$temporary/references.current" "$temporary/retained.current"
    cat "$temporary/containers.current" "$temporary/retained.current" |
      LC_ALL=C sort -u >"$temporary/protected.current"
    select_obsolete_weather_image_ids \
      "$temporary/images.current" "$temporary/repositories.current" \
      "$temporary/references.current" "$temporary/protected.current" \
      >"$temporary/candidates.current"
    write_image_alias_snapshot \
      "$temporary/images.current" "$image_id" "$temporary/aliases.current"

    # refuse a newly protected, removed, retagged or foreign-aliased candidate
    if ! grep -Fxq -- "$image_id" "$temporary/candidates.current" ||
      ! cmp -s "$temporary/aliases.before" "$temporary/aliases.current"; then
      die "Docker image cleanup candidate changed during per-image selection"
    fi
    docker image rm "$image_id" >/dev/null ||
      die "obsolete Weather image removal failed"
  done
  printf 'Removed %s obsolete Weather image(s).\n' "${#candidates[@]}" >&2
)

# resolve one explicit linux arm64 manifest
resolve_arm64_image() {
  local image=$1
  local resolved

  # preserve pinned manifests after platform verification
  if [[ "$image" == *@sha256:* ]]; then
    resolved=$(docker manifest inspect --verbose "$image" |
      node "$deploy_dir/scripts/resolve-image.mjs" "$image")
  else
    resolved=$(docker buildx imagetools inspect "$image" --raw |
      node "$deploy_dir/scripts/resolve-image.mjs" "$image")
  fi
  validate_image_reference "$resolved"
  printf '%s\n' "$resolved"
}

# validate one immutable production release environment
validate_release_env() {
  local path=$1
  local expected_release=${2:-}
  local release database_name postgres_dir control_plane control_version wind_canary_kill_switch temperature_canary_kill_switch
  local meaningful_lines allowed_fields
  require_file "$path"
  [[ ! -L "$path" ]] || die "release environment must not be a symbolic link: $path"
  meaningful_lines=$(grep -cE '^[A-Z][A-Z0-9_]*=' "$path")
  allowed_fields='^(WEATHER_RELEASE|WEATHER_SERVER_IMAGE|WEATHER_WEB_IMAGE|POSTGRES_IMAGE|CLOUDFLARED_IMAGE|WEATHER_DATABASE_NAME|WEATHER_POSTGRES_DIR|WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH|WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH|WEATHER_CONTROL_PLANE_SHA256|WEATHER_CONTROL_PLANE_VERSION)='
  control_plane=$(env_value "$path" WEATHER_CONTROL_PLANE_SHA256)
  control_version=$(env_value "$path" WEATHER_CONTROL_PLANE_VERSION)

  # preserve blank separators in the exact retained wind-only release
  if [[ "$control_version" == "$legacy_control_plane_version" && "$control_plane" == "$legacy_control_plane_sha256" ]]; then
    allowed_fields+='|^$'
  fi
  grep -qEv "$allowed_fields" "$path" &&
    die "release environment contains an unknown or malformed value"

  # accept only the exact field counts for current and legacy releases
  if [[ "$meaningful_lines" -eq 11 ]]; then
    temperature_canary_kill_switch=$(env_value "$path" WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH)
    [[ "$temperature_canary_kill_switch" =~ ^[01]$ ]] ||
      die "temperature canary kill switch must be 0 or 1"
    wind_canary_kill_switch=$(env_value "$path" WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH)
    [[ "$wind_canary_kill_switch" =~ ^[01]$ ]] ||
      die "wind canary kill switch must be 0 or 1"
  elif [[ "$meaningful_lines" -ne 10 || "$control_version" != "$legacy_control_plane_version" || "$control_plane" != "$legacy_control_plane_sha256" ]]; then
    die "release environment must contain the exact current or allowlisted legacy control-plane format"
  else
    # retain the actual version-six wind-only release format
    wind_canary_kill_switch=$(env_value "$path" WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH)
    [[ "$wind_canary_kill_switch" =~ ^[01]$ ]] ||
      die "wind canary kill switch must be 0 or 1"
  fi
  release=$(env_value "$path" WEATHER_RELEASE)
  validate_release "$release"

  # require the requested release identity
  if [[ -n "$expected_release" && "$release" != "$expected_release" ]]; then
    die "release environment identity mismatch"
  fi

  validate_image_reference "$(env_value "$path" WEATHER_SERVER_IMAGE)"
  validate_image_reference "$(env_value "$path" WEATHER_WEB_IMAGE)"
  validate_image_reference "$(env_value "$path" POSTGRES_IMAGE)"
  validate_image_reference "$(env_value "$path" CLOUDFLARED_IMAGE)"
  database_name=$(env_value "$path" WEATHER_DATABASE_NAME)
  postgres_dir=$(env_value "$path" WEATHER_POSTGRES_DIR)
  validate_database_name "$database_name"
  require_canonical_descendant "$postgres_dir" /var/lib/weather "PostgreSQL directory"
  [[ "$control_plane" =~ ^[a-f0-9]{64}$ ]] ||
    die "invalid deployment control-plane digest"
  [[ "$control_version" =~ ^[1-9][0-9]*$ ]] || die "invalid deployment control-plane version"
}

# validate one web-only environment against its exact core release
validate_dashboard_release_env() {
  local path=$1
  local expected_release=$2
  local expected_core=$3
  local core_env meaningful_lines allowed_fields key
  require_canonical_descendant "$path" "$releases_dir" "dashboard release environment"
  require_file "$path"
  [[ ! -L "$path" ]] || die "dashboard release environment must not be a symbolic link"
  [[ "$(stat -c '%u %a' "$path")" == "$EUID 600" ]] ||
    die "dashboard release environment must be owner-private"
  meaningful_lines=$(grep -cE '^[A-Z][A-Z0-9_]*=' "$path")
  allowed_fields='^(WEATHER_RELEASE|WEATHER_CORE_RELEASE|WEATHER_SERVER_IMAGE|WEATHER_WEB_IMAGE|POSTGRES_IMAGE|CLOUDFLARED_IMAGE|WEATHER_DATABASE_NAME|WEATHER_POSTGRES_DIR|WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH|WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH|WEATHER_CONTROL_PLANE_SHA256|WEATHER_CONTROL_PLANE_VERSION)='
  [[ "$meaningful_lines" -eq 12 ]] ||
    die "dashboard release environment must contain the exact web-only format"
  grep -qEv "$allowed_fields" "$path" &&
    die "dashboard release environment contains an unknown or malformed value"
  [[ "$(env_value "$path" WEATHER_RELEASE)" == "$expected_release" &&
    "$(env_value "$path" WEATHER_CORE_RELEASE)" == "$expected_core" ]] ||
    die "dashboard release environment identity mismatch"
  validate_release "$expected_release"
  validate_release "$expected_core"
  validate_image_reference "$(env_value "$path" WEATHER_WEB_IMAGE)"
  core_env=$(release_env "$expected_core")
  validate_release_env "$core_env" "$expected_core"

  # preserve every non-web core setting byte-for-value
  for key in WEATHER_SERVER_IMAGE POSTGRES_IMAGE CLOUDFLARED_IMAGE \
    WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH \
    WEATHER_CONTROL_PLANE_SHA256 WEATHER_CONTROL_PLANE_VERSION; do
    [[ "$(env_value "$path" "$key")" == "$(env_value "$core_env" "$key")" ]] ||
      die "dashboard release changes core setting: $key"
  done
}

# render one private web-only environment from the immutable core
write_dashboard_release_env() (
  local core_env=$1
  local target=$2
  local release=$3
  local web_image=$4
  local core_release key temporary
  local -a lines
  core_release=$(env_value "$core_env" WEATHER_RELEASE)
  validate_release "$release"
  validate_image_reference "$web_image"
  require_canonical_descendant "$target" "$releases_dir" "dashboard release environment"
  [[ ! -e "$target" && ! -L "$target" ]] ||
    die "dashboard release environment already exists"
  lines=(
    "WEATHER_RELEASE=$release"
    "WEATHER_CORE_RELEASE=$core_release"
  )

  # copy only the closed core environment contract
  for key in WEATHER_SERVER_IMAGE WEATHER_WEB_IMAGE POSTGRES_IMAGE CLOUDFLARED_IMAGE \
    WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH \
    WEATHER_CONTROL_PLANE_SHA256 WEATHER_CONTROL_PLANE_VERSION; do
    if [[ "$key" == WEATHER_WEB_IMAGE ]]; then
      lines+=("WEATHER_WEB_IMAGE=$web_image")
    else
      lines+=("$key=$(env_value "$core_env" "$key")")
    fi
  done
  umask 077
  temporary=$(mktemp "$releases_dir/.${release}.XXXXXX.web.env.partial")

  # remove an interrupted unpublished environment
  trap 'rm -f "$temporary"' EXIT
  printf '%s\n' "${lines[@]}" >"$temporary"
  chmod 600 "$temporary"
  validate_dashboard_release_env "$temporary" "$release" "$core_release"
  sync -f "$temporary"
  ln "$temporary" "$target" || die "dashboard release environment already exists"
  sync -f "$releases_dir"
  rm -f -- "$temporary"
  sync -f "$releases_dir"
  trap - EXIT
  validate_dashboard_release_env "$target" "$release" "$core_release"
)

# publish the selected web environment atomically
write_dashboard_active_symlink() {
  (
  local release=$1
  local path="$state_dir/active-web.env"
  local temporary_directory
  validate_release "$release"
  temporary_directory=$(mktemp -d "$state_dir/.active-web.env.XXXXXX")

  # remove interrupted link writes
  trap 'rm -rf "$temporary_directory"' EXIT
  ln -s "../releases/$release.web.env" "$temporary_directory/active-web.env"
  mv -Tf "$temporary_directory/active-web.env" "$path"
  sync -f "$state_dir"
  rmdir "$temporary_directory"
  sync -f "$state_dir"
  trap - EXIT
  )
}

# remove one optional dashboard marker durably
remove_dashboard_state() {
  local path=$1
  [[ "$path" == "$state_dir/current-web-release" ||
    "$path" == "$state_dir/previous-web-release" ||
    "$path" == "$state_dir/active-web.env" ||
    "$path" == "$dashboard_release_journal" ]] ||
    die "dashboard state path is outside its fixed allowlist"
  rm -f -- "$path"
  sync -f "$state_dir"
}

# resolve and validate the effective web source under one core
dashboard_source_environment() {
  local core_release=$1
  local expected_source=$2
  local current_web source_env expected_link
  current_web=$(read_optional_release_state "$state_dir/current-web-release")

  # fall back only to the exact committed core web
  if [[ -z "$current_web" ]]; then
    [[ "$expected_source" == "$core_release" ]] ||
      die "dashboard web source CAS mismatch"
    [[ ! -e "$state_dir/active-web.env" && ! -L "$state_dir/active-web.env" &&
      ! -e "$state_dir/previous-web-release" && ! -L "$state_dir/previous-web-release" ]] ||
      die "dashboard web markers are incomplete"
    release_env "$core_release"
    return
  fi

  [[ "$current_web" == "$expected_source" ]] ||
    die "dashboard web source CAS mismatch"
  source_env=$(dashboard_release_env "$current_web")
  validate_dashboard_release_env "$source_env" "$current_web" "$core_release"
  expected_link="../releases/$current_web.web.env"
  [[ -L "$state_dir/active-web.env" &&
    "$(readlink "$state_dir/active-web.env")" == "$expected_link" ]] ||
    die "active dashboard environment does not match committed state"
  printf '%s\n' "$source_env"
}

# bind a dashboard transaction to the unchanged live core
require_dashboard_core_cas() {
  local expected_core=$1
  local current schema active core_env
  [[ "$expected_core" == "$recurring_previous_release" ]] ||
    die "dashboard release requires the exact allowlisted core release"
  current=$(read_release_state "$state_dir/current-release")
  schema=$(read_release_state "$state_dir/schema-release")
  [[ "$current" == "$expected_core" && "$schema" == "$expected_core" ]] ||
    die "dashboard core or schema CAS mismatch"
  active="$state_dir/active.env"
  [[ -L "$active" && "$(readlink "$active")" == "../releases/$expected_core.env" ]] ||
    die "active core environment does not match committed state"
  core_env=$(release_env "$expected_core")
  validate_release_env "$core_env" "$expected_core"
  require_control_plane_compatibility "$core_env"
}

# inherit one independent canary switch without accepting malformed declarations
source_canary_kill_switch() {
  local source_env=$1
  local name=$2
  local default_value=$3
  local value
  local -a declarations
  [[ "$name" == WIND || "$name" == TEMPERATURE ]] || die "unknown canary switch"
  mapfile -t declarations < <(
    grep -E "^WEATHER_FORECAST_ADJUSTMENT_${name}_CANARY_KILL_SWITCH([=[:space:]]|$)" "$source_env" || true
  )

  # default only an omitted source field
  if ((${#declarations[@]} == 0)); then
    printf '%s\n' "$default_value"
    return
  fi

  ((${#declarations[@]} == 1)) || die "expected exactly one $name canary kill switch in $source_env"
  value=${declarations[0]#*=}
  [[ "${declarations[0]}" == "WEATHER_FORECAST_ADJUSTMENT_${name}_CANARY_KILL_SWITCH="* && "$value" =~ ^[01]$ ]] ||
    die "$name canary kill switch must be 0 or 1"
  printf '%s\n' "$value"
}

# require the installed deployment contract
require_control_plane_compatibility() {
  local env_file=$1
  local expected_version expected_digest expected_release current_digest
  grep -q '^WEATHER_CONTROL_PLANE_VERSION=' "$env_file" ||
    die "release state lacks deployment control-plane version metadata"
  grep -q '^WEATHER_CONTROL_PLANE_SHA256=' "$env_file" ||
    die "release state lacks deployment control-plane digest metadata"
  expected_version=$(env_value "$env_file" WEATHER_CONTROL_PLANE_VERSION)
  expected_digest=$(env_value "$env_file" WEATHER_CONTROL_PLANE_SHA256)
  current_digest=$(control_plane_digest)

  # accept the exact installed contract
  if [[ "$expected_version" == "$control_plane_version" && "$expected_digest" == "$current_digest" ]]; then
    return
  fi

  # accept only the complete pinned recurring-maintenance predecessor
  if [[ "$expected_version" == 13 &&
    "$expected_digest" == "$recurring_previous_control_plane_sha256" ]]; then
    expected_release=$(env_value "$env_file" WEATHER_RELEASE)
    [[ "$expected_release" == "$recurring_previous_release" ]] ||
      die "deployment control-plane identity is unsupported without an exact versioned allowlisted handoff"
    return
  fi

  die "deployment control-plane identity is unsupported without an exact versioned allowlisted handoff"
}

# write one deterministic release environment
write_release_env() {
  local source_env=$1
  local target=$2
  local release=$3
  local server_image=$4
  local web_image=$5
  local postgres_image=$6
  local cloudflared_image=$7
  local database_name postgres_dir control_plane wind_canary_kill_switch temperature_canary_kill_switch
  database_name=$(env_value "$source_env" WEATHER_DATABASE_NAME)
  postgres_dir=$(env_value "$source_env" WEATHER_POSTGRES_DIR)
  wind_canary_kill_switch=$(source_canary_kill_switch "$source_env" WIND 0)
  temperature_canary_kill_switch=$(source_canary_kill_switch "$source_env" TEMPERATURE 1)
  control_plane=$(control_plane_digest)
  umask 077
  printf '%s\n' \
    "WEATHER_RELEASE=$release" \
    "WEATHER_SERVER_IMAGE=$server_image" \
    "WEATHER_WEB_IMAGE=$web_image" \
    "POSTGRES_IMAGE=$postgres_image" \
    "CLOUDFLARED_IMAGE=$cloudflared_image" \
    "WEATHER_DATABASE_NAME=$database_name" \
    "WEATHER_POSTGRES_DIR=$postgres_dir" \
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=$wind_canary_kill_switch" \
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=$temperature_canary_kill_switch" \
    "WEATHER_CONTROL_PLANE_SHA256=$control_plane" \
    "WEATHER_CONTROL_PLANE_VERSION=$control_plane_version" >"$target"
  chmod 600 "$target"
  validate_release_env "$target" "$release"
}

# require recent passing preflight evidence
require_capacity_gate() {
  require_file "$capacity_evidence"
  node --input-type=module - "$capacity_evidence" <<'NODE'
import { readFile } from "node:fs/promises";

const path = process.argv[2];
const evidence = JSON.parse(await readFile(path, "utf8"));
const capturedAt = Date.parse(evidence.capturedAt);
const ageMilliseconds = Date.now() - capturedAt;

// require a recent full production sample
if (
  evidence.pass !== true ||
  evidence.sampleSeconds < 900 ||
  evidence.architecture?.host !== "aarch64" ||
  evidence.architecture?.docker !== "aarch64" ||
  !Number.isFinite(capturedAt) ||
  ageMilliseconds < -300_000 ||
  ageMilliseconds > 3_600_000
) {
  throw new Error("capacity evidence is absent, stale, incomplete, or failed");
}
NODE
}

# require fresh non-storage resource proof for the fixed v13 bridge
require_v13_resource_gate() {
  require_file "$v13_resource_evidence"
  node --input-type=module - "$v13_resource_evidence" <<'NODE'
import { lstatSync, readFileSync } from "node:fs";

const path = process.argv[2];
const details = lstatSync(path);

// require one bounded private regular evidence file
if (!details.isFile() || details.isSymbolicLink() || details.size < 1 ||
  details.size > 65_536 || (details.mode & 0o077) !== 0) {
  throw new Error("v13 resource evidence file is invalid");
}
const evidence = JSON.parse(readFileSync(path, "utf8"));
const exactKeys = (value, keys) => value !== null && typeof value === "object" &&
  !Array.isArray(value) &&
  JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort());
const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const finite = (value) => Number.isFinite(value) && value >= 0;

// reject schema drift before evaluating individual resources
if (!exactKeys(evidence, [
  "architecture", "capturedAt", "cpu", "docker", "load15",
  "memoryAvailableBytes", "pass", "sampleSeconds", "swapBytesPerMinute",
  "varLib",
]) ||
  !exactKeys(evidence.architecture, ["docker", "expected", "host"]) ||
  !exactKeys(evidence.cpu, ["actual", "minimum"]) ||
  !exactKeys(evidence.load15, ["actual", "maximum"]) ||
  !exactKeys(evidence.memoryAvailableBytes, ["minimumObserved", "minimumRequired"]) ||
  !exactKeys(evidence.swapBytesPerMinute, ["actual", "maximum"]) ||
  !exactKeys(evidence.varLib, [
    "databaseBytes", "freeBytes", "inodeFreePercent", "minimumFreeBytes",
  ]) ||
  !exactKeys(evidence.docker, [
    "freeBytes", "inodeFreePercent", "minimumFreeBytes", "root",
  ])) {
  throw new Error("v13 resource evidence schema is invalid");
}
const capturedAt = Date.parse(evidence.capturedAt);
const ageMilliseconds = Date.now() - capturedAt;
const expectedVarLibMinimum = Math.max(
  10 * 1_024 * 1_024 * 1_024,
  3 * evidence.varLib.databaseBytes + 5 * 1_024 * 1_024 * 1_024,
);
const expectedLoadMaximum = Number((evidence.cpu.actual * 0.5).toFixed(3));

// preserve every fresh architecture, CPU, load, memory, swap and inode gate
if (evidence.sampleSeconds < 900 || !integer(evidence.sampleSeconds) ||
  !Number.isFinite(capturedAt) || ageMilliseconds < -300_000 ||
  ageMilliseconds > 3_600_000 ||
  evidence.architecture.host !== "aarch64" ||
  evidence.architecture.docker !== "aarch64" ||
  evidence.architecture.expected !== "aarch64" ||
  evidence.cpu.minimum !== 4 || !integer(evidence.cpu.actual) ||
  evidence.cpu.actual < evidence.cpu.minimum ||
  !finite(evidence.load15.actual) || !finite(evidence.load15.maximum) ||
  evidence.load15.maximum !== expectedLoadMaximum ||
  evidence.load15.actual > evidence.load15.maximum ||
  evidence.memoryAvailableBytes.minimumRequired !== 1_792 * 1_024 * 1_024 ||
  !integer(evidence.memoryAvailableBytes.minimumObserved) ||
  evidence.memoryAvailableBytes.minimumObserved <
    evidence.memoryAvailableBytes.minimumRequired ||
  evidence.swapBytesPerMinute.maximum !== 1_024 * 1_024 ||
  !integer(evidence.swapBytesPerMinute.actual) ||
  evidence.swapBytesPerMinute.actual > evidence.swapBytesPerMinute.maximum ||
  !integer(evidence.varLib.databaseBytes) || !integer(evidence.varLib.freeBytes) ||
  !integer(evidence.varLib.minimumFreeBytes) ||
  evidence.varLib.minimumFreeBytes !== expectedVarLibMinimum ||
  !integer(evidence.varLib.inodeFreePercent) ||
  evidence.varLib.inodeFreePercent < 10 ||
  evidence.varLib.inodeFreePercent > 100 ||
  evidence.docker.root !== "/var/lib/docker" ||
  !integer(evidence.docker.freeBytes) ||
  evidence.docker.minimumFreeBytes !== 4 * 1_024 * 1_024 * 1_024 ||
  !integer(evidence.docker.inodeFreePercent) ||
  evidence.docker.inodeFreePercent < 10 ||
  evidence.docker.inodeFreePercent > 100) {
  throw new Error("v13 non-storage resource evidence is absent, stale, or failed");
}
const failedBuckets = [];

// identify only the obsolete storage formulas in the reviewed sample
if (evidence.varLib.freeBytes < evidence.varLib.minimumFreeBytes) {
  failedBuckets.push("varLib.storage");
}
if (evidence.docker.freeBytes < evidence.docker.minimumFreeBytes) {
  failedBuckets.push("docker.storage");
}
const fullyPassing = evidence.pass === true && failedBuckets.length === 0;
const reviewedStorageFailure = evidence.pass === false &&
  JSON.stringify(failedBuckets) ===
    JSON.stringify(["varLib.storage", "docker.storage"]);

// admit either a full pass or only the exact reviewed obsolete storage failures
if (!fullyPassing && !reviewedStorageFailure) {
  throw new Error("v13 resource evidence does not contain only the reviewed storage failures");
}
NODE
}

# require fresh non-storage proof for a normal v15 release
require_normal_release_resource_gate() {
  require_file "$v13_resource_evidence"
  node --input-type=module - "$v13_resource_evidence" <<'NODE'
import { lstatSync, readFileSync } from "node:fs";

const path = process.argv[2];
const details = lstatSync(path);

// require one bounded private regular evidence file
if (!details.isFile() || details.isSymbolicLink() || details.size < 1 ||
  details.size > 65_536 || (details.mode & 0o077) !== 0) {
  throw new Error("normal release resource evidence file is invalid");
}
const evidence = JSON.parse(readFileSync(path, "utf8"));
const exactKeys = (value, keys) => value !== null && typeof value === "object" &&
  !Array.isArray(value) &&
  JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort());
const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const finite = (value) => Number.isFinite(value) && value >= 0;

// reject schema drift before evaluating individual resources
if (!exactKeys(evidence, [
  "architecture", "capturedAt", "cpu", "docker", "load15",
  "memoryAvailableBytes", "pass", "sampleSeconds", "swapBytesPerMinute",
  "varLib",
]) ||
  !exactKeys(evidence.architecture, ["docker", "expected", "host"]) ||
  !exactKeys(evidence.cpu, ["actual", "minimum"]) ||
  !exactKeys(evidence.load15, ["actual", "maximum"]) ||
  !exactKeys(evidence.memoryAvailableBytes, ["minimumObserved", "minimumRequired"]) ||
  !exactKeys(evidence.swapBytesPerMinute, ["actual", "maximum"]) ||
  !exactKeys(evidence.varLib, [
    "databaseBytes", "freeBytes", "inodeFreePercent", "minimumFreeBytes",
  ]) ||
  !exactKeys(evidence.docker, [
    "freeBytes", "inodeFreePercent", "minimumFreeBytes", "root",
  ])) {
  throw new Error("normal release resource evidence schema is invalid");
}
const capturedAt = Date.parse(evidence.capturedAt);
const ageMilliseconds = Date.now() - capturedAt;
const expectedVarLibMinimum = Math.max(
  10 * 1_024 * 1_024 * 1_024,
  3 * evidence.varLib.databaseBytes + 5 * 1_024 * 1_024 * 1_024,
);
const expectedLoadMaximum = Number((evidence.cpu.actual * 0.5).toFixed(3));

// preserve every fresh architecture, CPU, load, memory, swap and inode gate
if (evidence.sampleSeconds < 900 || !integer(evidence.sampleSeconds) ||
  !Number.isFinite(capturedAt) || ageMilliseconds < -300_000 ||
  ageMilliseconds > 3_600_000 ||
  evidence.architecture.host !== "aarch64" ||
  evidence.architecture.docker !== "aarch64" ||
  evidence.architecture.expected !== "aarch64" ||
  evidence.cpu.minimum !== 4 || !integer(evidence.cpu.actual) ||
  evidence.cpu.actual < evidence.cpu.minimum ||
  !finite(evidence.load15.actual) || !finite(evidence.load15.maximum) ||
  evidence.load15.maximum !== expectedLoadMaximum ||
  evidence.load15.actual > evidence.load15.maximum ||
  evidence.memoryAvailableBytes.minimumRequired !== 1_792 * 1_024 * 1_024 ||
  !integer(evidence.memoryAvailableBytes.minimumObserved) ||
  evidence.memoryAvailableBytes.minimumObserved <
    evidence.memoryAvailableBytes.minimumRequired ||
  evidence.swapBytesPerMinute.maximum !== 1_024 * 1_024 ||
  !integer(evidence.swapBytesPerMinute.actual) ||
  evidence.swapBytesPerMinute.actual > evidence.swapBytesPerMinute.maximum ||
  !integer(evidence.varLib.databaseBytes) || !integer(evidence.varLib.freeBytes) ||
  !integer(evidence.varLib.minimumFreeBytes) ||
  evidence.varLib.minimumFreeBytes !== expectedVarLibMinimum ||
  !integer(evidence.varLib.inodeFreePercent) ||
  evidence.varLib.inodeFreePercent < 10 ||
  evidence.varLib.inodeFreePercent > 100 ||
  evidence.docker.root !== "/var/lib/docker" ||
  !integer(evidence.docker.freeBytes) ||
  evidence.docker.minimumFreeBytes !== 4 * 1_024 * 1_024 * 1_024 ||
  !integer(evidence.docker.inodeFreePercent) ||
  evidence.docker.inodeFreePercent < 10 ||
  evidence.docker.inodeFreePercent > 100) {
  throw new Error("normal release non-storage resource evidence is absent, stale, or failed");
}
const failedBuckets = [];

// identify only the obsolete storage formulas in the reviewed sample
if (evidence.varLib.freeBytes < evidence.varLib.minimumFreeBytes) {
  failedBuckets.push("varLib.storage");
}
if (evidence.docker.freeBytes < evidence.docker.minimumFreeBytes) {
  failedBuckets.push("docker.storage");
}
const fullyPassing = evidence.pass === true && failedBuckets.length === 0;
const reviewedStorageFailure = evidence.pass === false &&
  JSON.stringify(failedBuckets) === JSON.stringify(["varLib.storage"]);

// admit either a full pass or only the exact reviewed obsolete varLib storage failure
if (!fullyPassing && !reviewedStorageFailure) {
  throw new Error("normal release resource evidence does not contain only the reviewed storage failures");
}
NODE
}

# render one canonical measured normal-stage capacity request
write_normal_stage_capacity_input() {
  local source_env=$1
  local target_env=$2
  local phase=$3
  local output=$4
  local database_name postgres_dir source_release target_release source_history
  local system_identifier database_oid database_bytes largest_index_bytes max_wal_bytes
  local postgres_container_id source_database_file_count source_env_sha256 target_env_sha256
  database_name=$(env_value "$source_env" WEATHER_DATABASE_NAME)
  postgres_dir=$(env_value "$source_env" WEATHER_POSTGRES_DIR)
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)
  source_history=$(migration_history_sha256 "$source_env" "$database_name")
  [[ "$source_history" == adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93 ]] ||
    die "normal capacity source is not the exact schema-21 ledger"
  read -r system_identifier database_oid database_bytes largest_index_bytes max_wal_bytes < <(
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
        --tuples-only --no-align --field-separator=' ' --command \
        "SELECT (SELECT system_identifier FROM pg_control_system()), (SELECT oid FROM pg_database WHERE datname = current_database()), pg_database_size(current_database()), COALESCE((SELECT max(pg_relation_size(indexrelid)) FROM pg_index JOIN pg_class ON pg_class.oid = pg_index.indrelid WHERE pg_class.relnamespace NOT IN ('pg_catalog'::regnamespace, 'information_schema'::regnamespace)), 0), pg_size_bytes(current_setting('max_wal_size'))")
  postgres_container_id=$(WEATHER_ENV_FILE=$source_env compose ps -q postgres)
  [[ "$postgres_container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "normal capacity PostgreSQL container identity is invalid"
  [[ -d "$postgres_dir/base/$database_oid" && ! -L "$postgres_dir/base/$database_oid" ]] ||
    die "normal capacity database file root is unavailable"
  source_database_file_count=$(find -P "$postgres_dir/base/$database_oid" -type f -printf . | wc -c)
  [[ "$source_database_file_count" =~ ^[1-9][0-9]*$ ]] ||
    die "normal capacity database file count is invalid"
  source_env_sha256=$(sha256sum "$source_env" | awk '{print $1}')
  target_env_sha256=$(sha256sum "$target_env" | awk '{print $1}')
  node --input-type=module - "$output" "$phase" \
    "normal-${source_release//./-}-to-${target_release//./-}" \
    "$source_release" "$target_release" "$source_env_sha256" "$target_env_sha256" \
    "$system_identifier" "$database_oid" "$database_bytes" "$largest_index_bytes" \
    "$max_wal_bytes" "$source_database_file_count" "$postgres_dir" \
    "${WEATHER_COMPOSE_PROJECT_NAME:-weather}" "$postgres_container_id" \
    "$(env_value "$source_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$source_env" WEATHER_WEB_IMAGE)" \
    "$(env_value "$target_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$target_env" WEATHER_WEB_IMAGE)" <<'NODE'
import { writeFileSync } from "node:fs";

const [output, phase, operationId, sourceRelease, targetRelease, sourceEnvSha256,
  targetEnvSha256, systemIdentifier, sourceDatabaseOid, activeDatabaseBytes,
  largestSourceIndexBytes, maxWalBytes, sourceDatabaseFileCount, weatherPostgresDir,
  postgresComposeProject, postgresContainerId, sourceServer, sourceWeb, targetServer,
  targetWeb] = process.argv.slice(2);
const input = {
  activeDatabaseBytes: Number(activeDatabaseBytes),
  captureReserveBytes: 4_112_384,
  capturedAt: new Date().toISOString(),
  fixtureInodes: 4_096,
  imageReferences: [
    { reference: sourceServer, role: "source", runtime: "server" },
    { reference: sourceWeb, role: "source", runtime: "web" },
    { reference: targetServer, role: "target", runtime: "server" },
    { reference: targetWeb, role: "target", runtime: "web" },
  ],
  inodeFloor: 32_768,
  largestSourceIndexBytes: Number(largestSourceIndexBytes),
  maxWalBytes: Number(maxWalBytes),
  operationId,
  phase,
  postgresComposeProject,
  postgresContainerId,
  protectedFloorBytes: 2_030_043_136,
  retirementCreditBytes: 0,
  scratchBytes: 268_435_456,
  sourceDatabaseFileCount: Number(sourceDatabaseFileCount),
  sourceDatabaseOid: Number(sourceDatabaseOid),
  sourceEnvSha256,
  sourceLedgerSha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  sourceRelease,
  sourceSchemaVersion: 21,
  systemIdentifier,
  targetEnvSha256,
  targetLedgerSha256: "c8b43b714eb555027ea7439a813831ab7326ad951b800362f35b2343330da81f",
  targetRelease,
  targetSchemaVersion: 23,
  weatherPostgresDir,
};
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
writeFileSync(output, canonical(input), { mode: 0o600 });
NODE
  chmod 600 "$output"
}

# revalidate one ready receipt against its exact caller input and trusted current time
require_normal_capacity_receipt_binding() {
  local input=$1
  local receipt=$2
  local operation=$3
  local phase=$4
  [[ -f "$receipt" && ! -L "$receipt" && "$(stat -c '%u:%a' "$receipt")" == "$EUID:600" &&
    "$(stat -c '%s' "$receipt")" -ge 2 && "$(stat -c '%s' "$receipt")" -le 16384 ]] ||
    die "normal capacity receipt metadata is invalid"
  node --input-type=module - "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    "$input" "$receipt" "$operation" "$phase" <<'NODE'
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [modulePath, inputPath, receiptPath, operation, phase] = process.argv.slice(2);
const { validateNormalCapacityReceipt } = await import(pathToFileURL(modulePath));
const inputBytes = readFileSync(inputPath);
const receiptBytes = readFileSync(receiptPath);
const input = JSON.parse(inputBytes);
const receipt = JSON.parse(receiptBytes);
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);

// bind the immutable helper output to every exact request fact
if (!inputBytes.equals(Buffer.from(canonical(input))) ||
  !receiptBytes.equals(Buffer.from(canonical(receipt))) ||
  receipt.operation !== operation || receipt.phase !== phase ||
  receipt.state !== "capacity_ready" || receipt.reasons?.length !== 0 ||
  Object.keys(input).some((name) => canonical(receipt[name]) !== canonical(input[name]))) {
  throw new Error("normal capacity receipt binding differs");
}
validateNormalCapacityReceipt(receipt, new Date());
NODE
}

# collect and require one authoritative normal-stage capacity receipt
require_normal_stage_capacity() {
  local source_env=$1
  local target_env=$2
  local phase=$3
  local input=$4
  local receipt=$5
  write_normal_stage_capacity_input "$source_env" "$target_env" "$phase" "$input"
  if ! normal_release_capacity_receipt normal-stage-capacity "$input" >"$receipt"; then
    cat "$receipt" >&2
    die "normal stage capacity is blocked"
  fi
  chmod 600 "$receipt"
  require_normal_capacity_receipt_binding "$input" "$receipt" stage "$phase"
}

# seal the completed full-copy qualification for later activation
write_normal_stage_qualification_receipt() {
  local source_env=$1
  local target_env=$2
  local pre_pull_receipt=$3
  local post_pull_receipt=$4
  local peak_output=$5
  local output=$6
  node --input-type=module - "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    "$pre_pull_receipt" "$post_pull_receipt" "$peak_output" \
    "$output" <<'NODE'
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [modulePath, prePath, postPath, samplesPath, output] = process.argv.slice(2);
const { validateNormalCapacityReceipt } = await import(pathToFileURL(modulePath));
const pre = JSON.parse(readFileSync(prePath, "utf8"));
const post = JSON.parse(readFileSync(postPath, "utf8"));
const samplesBytes = readFileSync(samplesPath);
const samples = JSON.parse(samplesBytes);
const trustedNow = new Date();
validateNormalCapacityReceipt(pre, trustedNow);
validateNormalCapacityReceipt(post, trustedNow);
const bound = ["operationId", "sourceRelease", "targetRelease", "sourceEnvSha256",
  "targetEnvSha256", "systemIdentifier", "sourceDatabaseOid", "sourceLedgerSha256",
  "targetLedgerSha256", "sourceSchemaVersion", "targetSchemaVersion",
  "postgresContainerId", "postgresContainerImageId", "postgresContainerImageReference",
  "weatherPostgresDir", "postgresDataDirectory"];

// require the post-pull receipt to retain every immutable pre-pull binding
if (pre.operation !== "stage" || post.operation !== "stage" ||
  pre.phase !== "pre_pull" || post.phase !== "post_pull" ||
  pre.state !== "capacity_ready" || post.state !== "capacity_ready" ||
  bound.some((name) => pre[name] !== post[name])) {
  throw new Error("normal stage capacity receipts are not one transaction");
}
const sampleKeys = ["contractVersion", "initialFreeBytes", "initialFreeInodes",
  "minimumFreeBytes", "minimumFreeInodes", "peakGrowthBytes", "peakGrowthInodes",
  "migrationDurationMilliseconds", "migrationMinimumFreeBytes",
  "migrationMinimumFreeInodes", "migrationPeakGrowthBytes", "migrationPeakGrowthInodes",
  "migrationPhaseEndIndex", "migrationPhaseEndTimestamp", "migrationPhaseStartIndex",
  "migrationPhaseStartTimestamp", "receiptSha256", "samples"];
const sampleBody = { ...samples };
delete sampleBody.receiptSha256;
const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const sampleDigest = createHash("sha256").update(canonical(sampleBody)).digest("hex");
const byteHeadroom = post.freeBytes - post.protectedFloorBytes - post.captureReserveBytes;
const inodeHeadroom = post.freeInodes - post.inodeFloor;
const imagePullConsumedBytes = Math.max(0, pre.freeBytes - post.freeBytes);
const imagePullConsumedInodes = Math.max(0, pre.freeInodes - post.freeInodes);
let sampleSeriesValid = Array.isArray(samples.samples) && samples.samples.length >= 2 &&
  samples.samples.length <= 4_096;
// reject missing or reordered samples even when the outer digest was recomputed
if (sampleSeriesValid) {
  for (let index = 0; index < samples.samples.length; index += 1) {
    const sample = samples.samples[index];
    if (Object.keys(sample).sort().join("\n") !==
      ["capturedAtMilliseconds", "freeBytes", "freeInodes", "sequence"].sort().join("\n") ||
      sample.sequence !== index || !integer(sample.capturedAtMilliseconds) ||
      !integer(sample.freeBytes) || !integer(sample.freeInodes) ||
      (index > 0 && sample.capturedAtMilliseconds <
        samples.samples[index - 1].capturedAtMilliseconds)) {
      sampleSeriesValid = false;
      break;
    }
  }
}
const observedMinimumBytes = sampleSeriesValid
  ? Math.min(...samples.samples.map((sample) => sample.freeBytes)) : -1;
const observedMinimumInodes = sampleSeriesValid
  ? Math.min(...samples.samples.map((sample) => sample.freeInodes)) : -1;
const phaseIndicesValid = integer(samples.migrationPhaseStartIndex) &&
  integer(samples.migrationPhaseEndIndex) &&
  samples.migrationPhaseStartIndex < samples.migrationPhaseEndIndex &&
  samples.migrationPhaseEndIndex < samples.samples.length;
const migrationSamples = phaseIndicesValid
  ? samples.samples.slice(samples.migrationPhaseStartIndex,
    samples.migrationPhaseEndIndex + 1) : [];
const observedMigrationMinimumBytes = phaseIndicesValid
  ? Math.min(...migrationSamples.map((sample) => sample.freeBytes)) : -1;
const observedMigrationMinimumInodes = phaseIndicesValid
  ? Math.min(...migrationSamples.map((sample) => sample.freeInodes)) : -1;
const wholeStagePeakBytes = sampleSeriesValid
  ? Math.max(post.freeBytes, samples.initialFreeBytes) - observedMinimumBytes : -1;
const wholeStagePeakInodes = sampleSeriesValid
  ? Math.max(post.freeInodes, samples.initialFreeInodes) - observedMinimumInodes : -1;

// close the measured full-copy peak inside the admitted post-pull envelope
if (Object.keys(samples).sort().join("\n") !== sampleKeys.sort().join("\n") ||
  samples.contractVersion !== "normal-stage-capacity-samples/v1" ||
  !sampleSeriesValid || samples.receiptSha256 !== sampleDigest ||
  ![samples.initialFreeBytes, samples.initialFreeInodes, samples.minimumFreeBytes,
    samples.minimumFreeInodes, samples.peakGrowthBytes,
    samples.peakGrowthInodes, samples.migrationDurationMilliseconds,
    samples.migrationMinimumFreeBytes, samples.migrationMinimumFreeInodes,
    samples.migrationPeakGrowthBytes, samples.migrationPeakGrowthInodes,
    samples.migrationPhaseStartTimestamp, samples.migrationPhaseEndTimestamp].every(integer) ||
  !phaseIndicesValid ||
  samples.migrationDurationMilliseconds > 60_000 ||
  samples.minimumFreeBytes < post.protectedFloorBytes + post.captureReserveBytes ||
  samples.minimumFreeInodes < post.inodeFloor || byteHeadroom < 0 || inodeHeadroom < 0 ||
  samples.minimumFreeBytes !== observedMinimumBytes ||
  samples.minimumFreeInodes !== observedMinimumInodes ||
  samples.initialFreeBytes !== samples.samples[0]?.freeBytes ||
  samples.initialFreeInodes !== samples.samples[0]?.freeInodes ||
  samples.peakGrowthBytes !== samples.initialFreeBytes - observedMinimumBytes ||
  samples.peakGrowthInodes !== samples.initialFreeInodes - observedMinimumInodes ||
  samples.migrationPhaseStartTimestamp !==
    samples.samples[samples.migrationPhaseStartIndex]?.capturedAtMilliseconds ||
  samples.migrationPhaseEndTimestamp !==
    samples.samples[samples.migrationPhaseEndIndex]?.capturedAtMilliseconds ||
  samples.migrationMinimumFreeBytes !== observedMigrationMinimumBytes ||
  samples.migrationMinimumFreeInodes !== observedMigrationMinimumInodes ||
  samples.migrationPeakGrowthBytes !==
    samples.samples[samples.migrationPhaseStartIndex]?.freeBytes - observedMigrationMinimumBytes ||
  samples.migrationPeakGrowthInodes !==
    samples.samples[samples.migrationPhaseStartIndex]?.freeInodes - observedMigrationMinimumInodes ||
  imagePullConsumedBytes > pre.imagePullPeakBytes ||
  imagePullConsumedInodes > pre.newImageInodes ||
  wholeStagePeakBytes > byteHeadroom || wholeStagePeakInodes > inodeHeadroom) {
  throw new Error("normal compatibility peak exceeds its post-pull capacity envelope");
}
const body = {
  capacitySamplesSha256: createHash("sha256").update(samplesBytes).digest("hex"),
  capacitySamplesReceiptSha256: samples.receiptSha256,
  capacitySampleCount: samples.samples.length,
  cleanupConfirmed: true,
  contractVersion: "normal-stage-qualification/v1",
  createdAt: new Date().toISOString(),
  imagePullConsumedBytes,
  imagePullConsumedInodes,
  measuredMigrationPeakBytes: samples.migrationPeakGrowthBytes,
  measuredMigrationPeakInodes: samples.migrationPeakGrowthInodes,
  migrationMinimumFreeBytesObserved: samples.migrationMinimumFreeBytes,
  migrationMinimumFreeInodesObserved: samples.migrationMinimumFreeInodes,
  migrationPhaseEndIndex: samples.migrationPhaseEndIndex,
  migrationPhaseEndTimestamp: samples.migrationPhaseEndTimestamp,
  migrationPhaseStartIndex: samples.migrationPhaseStartIndex,
  migrationPhaseStartTimestamp: samples.migrationPhaseStartTimestamp,
  migrationDurationMilliseconds: samples.migrationDurationMilliseconds,
  minimumFreeBytesObserved: samples.minimumFreeBytes,
  minimumFreeInodesObserved: samples.minimumFreeInodes,
  operationId: post.operationId,
  postPullReceiptSha256: post.receiptSha256,
  postPullFreeBytes: post.freeBytes,
  postPullFreeInodes: post.freeInodes,
  prePullReceiptSha256: pre.receiptSha256,
  sourceDatabaseOid: post.sourceDatabaseOid,
  sourceEnvSha256: post.sourceEnvSha256,
  sourceLedgerSha256: post.sourceLedgerSha256,
  sourceRelease: post.sourceRelease,
  sourceSchemaVersion: post.sourceSchemaVersion,
  systemIdentifier: post.systemIdentifier,
  targetEnvSha256: post.targetEnvSha256,
  targetLedgerSha256: post.targetLedgerSha256,
  targetRelease: post.targetRelease,
  targetSchemaVersion: post.targetSchemaVersion,
  wholeStagePeakBytes,
  wholeStagePeakInodes,
};
function canonical(value) {
  return Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
}
const receipt = { ...body,
  receiptSha256: createHash("sha256").update(canonical(body)).digest("hex") };
writeFileSync(output, canonical(receipt), { mode: 0o600 });
NODE
  chmod 600 "$output"
}

# validate one immutable cleanup-confirmed stage qualification against live source identity
read_normal_stage_qualification() {
  local source_env=$1
  local target_env=$2
  local receipt=$3
  local database_name source_env_sha256 target_env_sha256 system_identifier database_oid
  local capacity_samples
  database_name=$(env_value "$source_env" WEATHER_DATABASE_NAME)
  require_canonical_descendant "$receipt" "$releases_dir" "normal stage qualification"
  require_file "$receipt"
  [[ ! -L "$receipt" && "$(stat -c '%u:%a' "$receipt")" == "$EUID:600" &&
    "$(stat -c '%s' "$receipt")" -le 16384 ]] ||
    die "normal stage qualification metadata is invalid"
  source_env_sha256=$(sha256sum "$source_env" | awk '{print $1}')
  target_env_sha256=$(sha256sum "$target_env" | awk '{print $1}')
  capacity_samples="$releases_dir/$(env_value "$target_env" WEATHER_RELEASE).normal-stage-samples.json"
  require_canonical_descendant "$capacity_samples" "$releases_dir" \
    "normal stage capacity samples"
  require_file "$capacity_samples"
  [[ ! -L "$capacity_samples" && "$(stat -c '%u:%a' "$capacity_samples")" == "$EUID:600" &&
    "$(stat -c '%s' "$capacity_samples")" -le 1048576 ]] ||
    die "normal stage capacity sample artifact is invalid"
  read -r system_identifier database_oid < <(
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
        --tuples-only --no-align --field-separator=' ' --command \
        "SELECT (SELECT system_identifier FROM pg_control_system()), (SELECT oid FROM pg_database WHERE datname = current_database())")
  node --input-type=module - "$receipt" "$capacity_samples" \
    "$source_env_sha256" "$target_env_sha256" \
    "$system_identifier" "$database_oid" \
    "$(env_value "$source_env" WEATHER_RELEASE)" \
    "$(env_value "$target_env" WEATHER_RELEASE)" <<'NODE'
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const [path, samplesPath, sourceEnvSha256, targetEnvSha256, systemIdentifier, databaseOid,
  sourceRelease, targetRelease] = process.argv.slice(2);
const receipt = JSON.parse(readFileSync(path, "utf8"));
const samplesBytes = readFileSync(samplesPath);
const samples = JSON.parse(samplesBytes);
const keys = ["capacitySampleCount", "capacitySamplesReceiptSha256",
  "capacitySamplesSha256", "cleanupConfirmed", "contractVersion", "createdAt",
  "imagePullConsumedBytes", "imagePullConsumedInodes",
  "measuredMigrationPeakBytes", "measuredMigrationPeakInodes", "operationId",
  "migrationDurationMilliseconds", "migrationMinimumFreeBytesObserved",
  "migrationMinimumFreeInodesObserved", "migrationPhaseEndIndex",
  "migrationPhaseEndTimestamp", "migrationPhaseStartIndex",
  "migrationPhaseStartTimestamp",
  "minimumFreeBytesObserved", "minimumFreeInodesObserved",
  "postPullFreeBytes", "postPullFreeInodes", "postPullReceiptSha256",
  "prePullReceiptSha256", "receiptSha256",
  "sourceDatabaseOid", "sourceEnvSha256", "sourceLedgerSha256", "sourceRelease",
  "sourceSchemaVersion", "systemIdentifier", "targetEnvSha256", "targetLedgerSha256",
  "targetRelease", "targetSchemaVersion", "wholeStagePeakBytes", "wholeStagePeakInodes"];
const body = { ...receipt };
delete body.receiptSha256;
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
const digest = createHash("sha256").update(canonical(body)).digest("hex");
const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const sampleBody = { ...samples };
delete sampleBody.receiptSha256;
const sampleDigest = createHash("sha256").update(canonical(sampleBody)).digest("hex");
const sampleKeys = ["contractVersion", "initialFreeBytes", "initialFreeInodes",
  "minimumFreeBytes", "minimumFreeInodes", "migrationDurationMilliseconds",
  "migrationMinimumFreeBytes", "migrationMinimumFreeInodes", "migrationPeakGrowthBytes",
  "migrationPeakGrowthInodes", "migrationPhaseEndIndex", "migrationPhaseEndTimestamp",
  "migrationPhaseStartIndex", "migrationPhaseStartTimestamp", "peakGrowthBytes",
  "peakGrowthInodes", "receiptSha256", "samples"];
let sampleSeriesValid = Object.keys(samples).sort().join("\n") ===
  sampleKeys.sort().join("\n") && Array.isArray(samples.samples) &&
  samples.samples.length >= 2 && samples.samples.length <= 4_096;
// rederive every phase boundary and peak from the retained complete series
if (sampleSeriesValid) {
  for (let index = 0; index < samples.samples.length; index += 1) {
    const sample = samples.samples[index];
    if (Object.keys(sample).sort().join("\n") !==
      ["capturedAtMilliseconds", "freeBytes", "freeInodes", "sequence"].sort().join("\n") ||
      sample.sequence !== index || !integer(sample.capturedAtMilliseconds) ||
      !integer(sample.freeBytes) || !integer(sample.freeInodes) ||
      (index > 0 && sample.capturedAtMilliseconds <
        samples.samples[index - 1].capturedAtMilliseconds)) {
      sampleSeriesValid = false;
      break;
    }
  }
}
const phaseIndicesValid = sampleSeriesValid && integer(samples.migrationPhaseStartIndex) &&
  integer(samples.migrationPhaseEndIndex) &&
  samples.migrationPhaseStartIndex < samples.migrationPhaseEndIndex &&
  samples.migrationPhaseEndIndex < samples.samples.length;
const migrationSamples = phaseIndicesValid
  ? samples.samples.slice(samples.migrationPhaseStartIndex,
    samples.migrationPhaseEndIndex + 1) : [];
const migrationMinimumFreeBytes = phaseIndicesValid
  ? Math.min(...migrationSamples.map((sample) => sample.freeBytes)) : -1;
const migrationMinimumFreeInodes = phaseIndicesValid
  ? Math.min(...migrationSamples.map((sample) => sample.freeInodes)) : -1;
const wholeMinimumFreeBytes = sampleSeriesValid
  ? Math.min(...samples.samples.map((sample) => sample.freeBytes)) : -1;
const wholeMinimumFreeInodes = sampleSeriesValid
  ? Math.min(...samples.samples.map((sample) => sample.freeInodes)) : -1;
if (Object.keys(receipt).sort().join("\n") !== keys.sort().join("\n") ||
  receipt.contractVersion !== "normal-stage-qualification/v1" ||
  receipt.cleanupConfirmed !== true || receipt.sourceEnvSha256 !== sourceEnvSha256 ||
  receipt.targetEnvSha256 !== targetEnvSha256 ||
  receipt.systemIdentifier !== systemIdentifier ||
  receipt.sourceDatabaseOid !== Number(databaseOid) ||
  receipt.sourceRelease !== sourceRelease || receipt.targetRelease !== targetRelease ||
  receipt.sourceLedgerSha256 !== "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424" ||
  receipt.targetLedgerSha256 !== "c8b43b714eb555027ea7439a813831ab7326ad951b800362f35b2343330da81f" ||
  receipt.sourceSchemaVersion !== 21 || receipt.targetSchemaVersion !== 23 ||
  !integer(receipt.measuredMigrationPeakBytes) ||
  !integer(receipt.measuredMigrationPeakInodes) ||
  !integer(receipt.migrationDurationMilliseconds) ||
  receipt.migrationDurationMilliseconds > 60_000 ||
  !integer(receipt.imagePullConsumedBytes) ||
  !integer(receipt.imagePullConsumedInodes) ||
  !integer(receipt.postPullFreeBytes) ||
  !integer(receipt.postPullFreeInodes) ||
  !integer(receipt.minimumFreeBytesObserved) ||
  !integer(receipt.minimumFreeInodesObserved) ||
  !integer(receipt.capacitySampleCount) ||
  !sampleSeriesValid || !phaseIndicesValid ||
  receipt.capacitySampleCount !== samples.samples?.length ||
  receipt.capacitySamplesSha256 !== createHash("sha256").update(samplesBytes).digest("hex") ||
  receipt.capacitySamplesReceiptSha256 !== samples.receiptSha256 ||
  samples.receiptSha256 !== sampleDigest ||
  receipt.measuredMigrationPeakBytes !== samples.migrationPeakGrowthBytes ||
  receipt.measuredMigrationPeakInodes !== samples.migrationPeakGrowthInodes ||
  receipt.migrationMinimumFreeBytesObserved !== migrationMinimumFreeBytes ||
  receipt.migrationMinimumFreeInodesObserved !== migrationMinimumFreeInodes ||
  receipt.migrationPhaseStartIndex !== samples.migrationPhaseStartIndex ||
  receipt.migrationPhaseEndIndex !== samples.migrationPhaseEndIndex ||
  receipt.migrationPhaseStartTimestamp !==
    samples.samples[samples.migrationPhaseStartIndex]?.capturedAtMilliseconds ||
  receipt.migrationPhaseEndTimestamp !==
    samples.samples[samples.migrationPhaseEndIndex]?.capturedAtMilliseconds ||
  samples.migrationPhaseStartTimestamp !== receipt.migrationPhaseStartTimestamp ||
  samples.migrationPhaseEndTimestamp !== receipt.migrationPhaseEndTimestamp ||
  samples.migrationMinimumFreeBytes !== migrationMinimumFreeBytes ||
  samples.migrationMinimumFreeInodes !== migrationMinimumFreeInodes ||
  samples.migrationPeakGrowthBytes !==
    samples.samples[samples.migrationPhaseStartIndex]?.freeBytes - migrationMinimumFreeBytes ||
  samples.migrationPeakGrowthInodes !==
    samples.samples[samples.migrationPhaseStartIndex]?.freeInodes - migrationMinimumFreeInodes ||
  receipt.wholeStagePeakBytes !==
    Math.max(receipt.postPullFreeBytes, samples.initialFreeBytes) - wholeMinimumFreeBytes ||
  receipt.wholeStagePeakInodes !==
    Math.max(receipt.postPullFreeInodes, samples.initialFreeInodes) - wholeMinimumFreeInodes ||
  samples.minimumFreeBytes !== wholeMinimumFreeBytes ||
  samples.minimumFreeInodes !== wholeMinimumFreeInodes ||
  samples.peakGrowthBytes !== samples.initialFreeBytes - wholeMinimumFreeBytes ||
  samples.peakGrowthInodes !== samples.initialFreeInodes - wholeMinimumFreeInodes ||
  receipt.migrationDurationMilliseconds !== samples.migrationDurationMilliseconds ||
  receipt.minimumFreeBytesObserved !== samples.minimumFreeBytes ||
  receipt.minimumFreeInodesObserved !== samples.minimumFreeInodes ||
  receipt.receiptSha256 !== digest) {
  throw new Error("normal stage qualification differs from the live source");
}
process.stdout.write(`${receipt.measuredMigrationPeakBytes} ${receipt.measuredMigrationPeakInodes} ${receipt.receiptSha256}\n`);
NODE
}

# render one canonical measured activation-capacity request
write_normal_activation_capacity_input() {
  local source_env=$1
  local target_env=$2
  local stage_receipt=$3
  local phase=$4
  local actual_backup_bytes=$5
  local output=$6
  local database_name source_release target_release source_history
  local system_identifier database_oid live_database_bytes postgres_container_id
  local source_env_sha256 target_env_sha256 peak_bytes peak_inodes stage_receipt_sha256
  database_name=$(env_value "$source_env" WEATHER_DATABASE_NAME)
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)
  source_history=$(migration_history_sha256 "$source_env" "$database_name")
  [[ "$source_history" == adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93 ]] ||
    die "normal activation source is not the exact schema-21 ledger"
  read -r peak_bytes peak_inodes stage_receipt_sha256 < <(
    read_normal_stage_qualification "$source_env" "$target_env" "$stage_receipt")
  read -r system_identifier database_oid live_database_bytes < <(
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
        --tuples-only --no-align --field-separator=' ' --command \
        "SELECT (SELECT system_identifier FROM pg_control_system()), (SELECT oid FROM pg_database WHERE datname = current_database()), pg_database_size(current_database())")
  postgres_container_id=$(WEATHER_ENV_FILE=$source_env compose ps -q postgres)
  [[ "$postgres_container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "normal activation PostgreSQL container identity is invalid"
  source_env_sha256=$(sha256sum "$source_env" | awk '{print $1}')
  target_env_sha256=$(sha256sum "$target_env" | awk '{print $1}')
  node --input-type=module - "$output" "$phase" \
    "normal-${source_release//./-}-to-${target_release//./-}" "$source_release" \
    "$target_release" "$source_env_sha256" "$target_env_sha256" "$system_identifier" \
    "$database_oid" "$live_database_bytes" "$peak_bytes" "$peak_inodes" \
    "$stage_receipt_sha256" "$actual_backup_bytes" \
    "$(env_value "$source_env" WEATHER_POSTGRES_DIR)" \
    "${WEATHER_COMPOSE_PROJECT_NAME:-weather}" "$postgres_container_id" <<'NODE'
import { writeFileSync } from "node:fs";

const [output, phase, operationId, sourceRelease, targetRelease, sourceEnvSha256,
  targetEnvSha256, systemIdentifier, sourceDatabaseOid, liveDatabaseBytes,
  measuredMigrationPeakBytes, measuredMigrationPeakInodes, stageReceiptSha256,
  actualBackupBytes, weatherPostgresDir, postgresComposeProject,
  postgresContainerId] = process.argv.slice(2);
const backupHeadroomBytes = 1_073_741_824;
const input = {
  actualBackupBytes: Number(actualBackupBytes),
  backupHeadroomBytes,
  boundedBackupCeilingBytes: Number(liveDatabaseBytes) + backupHeadroomBytes,
  captureReserveBytes: 4_112_384,
  capturedAt: new Date().toISOString(),
  fixtureInodes: 4_096,
  inodeFloor: 32_768,
  liveDatabaseBytes: Number(liveDatabaseBytes),
  measuredMigrationPeakBytes: Number(measuredMigrationPeakBytes),
  measuredMigrationPeakInodes: Number(measuredMigrationPeakInodes),
  operationId,
  phase,
  postgresComposeProject,
  postgresContainerId,
  protectedFloorBytes: 2_030_043_136,
  retirementCreditBytes: 0,
  sourceDatabaseOid: Number(sourceDatabaseOid),
  sourceEnvSha256,
  sourceLedgerSha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  sourceRelease,
  sourceSchemaVersion: 21,
  stageReceiptSha256,
  systemIdentifier,
  targetEnvSha256,
  targetLedgerSha256: "c8b43b714eb555027ea7439a813831ab7326ad951b800362f35b2343330da81f",
  targetRelease,
  targetSchemaVersion: 23,
  weatherPostgresDir,
};
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
writeFileSync(output, canonical(input), { mode: 0o600 });
NODE
  chmod 600 "$output"
}

# collect and require one authoritative normal-activation capacity receipt
require_normal_activation_capacity() {
  local source_env=$1
  local target_env=$2
  local stage_receipt=$3
  local phase=$4
  local actual_backup_bytes=$5
  local input=$6
  local receipt=$7
  write_normal_activation_capacity_input "$source_env" "$target_env" "$stage_receipt" \
    "$phase" "$actual_backup_bytes" "$input"
  if ! normal_release_capacity_receipt normal-activation-capacity "$input" >"$receipt"; then
    cat "$receipt" >&2
    die "normal activation capacity is blocked"
  fi
  chmod 600 "$receipt"
  require_normal_capacity_receipt_binding "$input" "$receipt" activation "$phase"
}

# bind pre/post backup capacity to only the two invocation-owned publication files
require_normal_activation_capacity_pair() {
  local pre_receipt=$1
  local post_receipt=$2
  local archive=$3
  local checksum=$4
  local allocated_bytes
  allocated_bytes=$((( $(stat -c '%b' "$archive") + $(stat -c '%b' "$checksum") ) * 512))
  node --input-type=module - "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    "$pre_receipt" "$post_receipt" "$allocated_bytes" <<'NODE'
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [modulePath, prePath, postPath, allocatedBytes] = process.argv.slice(2);
const { validateNormalCapacityReceipt } = await import(pathToFileURL(modulePath));
const trustedNow = new Date();
const pre = JSON.parse(readFileSync(prePath, "utf8"));
const post = JSON.parse(readFileSync(postPath, "utf8"));
validateNormalCapacityReceipt(pre, trustedNow);
validateNormalCapacityReceipt(post, trustedNow);
const bound = ["operationId", "sourceRelease", "targetRelease", "sourceEnvSha256",
  "targetEnvSha256", "sourceLedgerSha256", "targetLedgerSha256", "systemIdentifier",
  "sourceDatabaseOid", "stageReceiptSha256", "postgresContainerId",
  "postgresContainerImageId", "postgresContainerImageReference", "weatherPostgresDir",
  "postgresDataDirectory"];
const consumedBytes = Math.max(0, pre.freeBytes - post.freeBytes);
const consumedInodes = Math.max(0, pre.freeInodes - post.freeInodes);

// refuse identity drift or unrelated storage consumption across backup publication
if (pre.phase !== "pre_backup" || post.phase !== "post_backup" ||
  pre.state !== "capacity_ready" || post.state !== "capacity_ready" ||
  bound.some((name) => pre[name] !== post[name]) ||
  consumedBytes > Number(allocatedBytes) + 8_192 || consumedInodes > 2) {
  throw new Error("normal activation capacity changed outside the bounded backup");
}
NODE
}

# require host-owned per-consumer secrets
require_secret_source() {
  local path=$1
  local expected_uid=$2
  local expected_gid=$3
  local metadata uid gid mode
  require_canonical_descendant "$path" "$deploy_dir/secrets" "secret source"
  require_file "$path"
  metadata=$(stat -c '%u %g %a' "$path")
  read -r uid gid mode <<<"$metadata"
  [[ "$uid" == "$expected_uid" && "$gid" == "$expected_gid" ]] ||
    die "secret source has incorrect ownership: $path"
  [[ "$mode" == 400 ]] || die "secret source must use mode 0400: $path"
}

# verify every consumer copy without exposing material
require_deployment_secrets() {
  require_command cmp
  require_secret_source "$deploy_dir/secrets/weather_postgres_admin_password" 999 999
  require_secret_source "$deploy_dir/secrets/weather_postgres_owner_password" 999 999
  require_secret_source "$deploy_dir/secrets/weather_postgres_api_password" 999 999
  require_secret_source "$deploy_dir/secrets/weather_postgres_ingest_password" 999 999
  require_secret_source "$deploy_dir/secrets/weather_postgres_training_export_password" 999 999
  require_secret_source "$deploy_dir/secrets/weather_migration_owner_password" 10002 10002
  require_secret_source "$deploy_dir/secrets/weather_api_password" 10002 10002
  require_secret_source "$deploy_dir/secrets/weather_worker_ingest_password" 10002 10002
  require_secret_source "$deploy_dir/secrets/weather_tempest_api_key" 10002 10002
  require_secret_source "$deploy_dir/secrets/weather_xweather_client_id" 10002 10002
  require_secret_source "$deploy_dir/secrets/weather_xweather_client_secret" 10002 10002
  require_secret_source "$deploy_dir/secrets/cloudflare_tunnel_token" 65532 65532
  ! cmp -s "$deploy_dir/secrets/weather_postgres_admin_password" \
    "$deploy_dir/secrets/weather_postgres_owner_password" ||
    die "administrator and owner passwords must differ"
  cmp -s "$deploy_dir/secrets/weather_postgres_owner_password" \
    "$deploy_dir/secrets/weather_migration_owner_password" ||
    die "owner password copies differ"
  cmp -s "$deploy_dir/secrets/weather_postgres_api_password" \
    "$deploy_dir/secrets/weather_api_password" || die "API password copies differ"
  cmp -s "$deploy_dir/secrets/weather_postgres_ingest_password" \
    "$deploy_dir/secrets/weather_worker_ingest_password" ||
    die "ingest password copies differ"
  # reject shared training-export authority
  for runtime_secret in \
    weather_postgres_admin_password \
    weather_postgres_owner_password \
    weather_postgres_api_password \
    weather_postgres_ingest_password; do
    ! cmp -s "$deploy_dir/secrets/weather_postgres_training_export_password" \
      "$deploy_dir/secrets/$runtime_secret" ||
      die "training export password must differ from every runtime password"
  done
}

# record success with the current marker as commit point
record_release_success() {
  local target=$1
  local current=$2

  # retain the replaced release for rollback
  if [[ -n "$current" && "$current" != "$target" ]]; then
    write_private_state "$state_dir/previous-release" "$current"
  fi

  write_active_symlink "$target"
  write_private_state "$state_dir/current-release" "$target"
}

# require the one source image set reviewed for the inert v13 bridge
require_fixed_v13_source_identity() {
  local target_env=$1
  local source_env=$2
  local source_control source_control_version source_release source_server source_web
  local target_release target_version
  source_control=$(env_value "$source_env" WEATHER_CONTROL_PLANE_SHA256)
  source_control_version=$(env_value "$source_env" WEATHER_CONTROL_PLANE_VERSION)
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  source_server=$(env_value "$source_env" WEATHER_SERVER_IMAGE)
  source_web=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)
  target_version=$(env_value "$target_env" WEATHER_CONTROL_PLANE_VERSION)

  [[ "$source_release" == "2026.10.07-3" ]] ||
    die "fixed v13 source compatibility requires release 2026.10.07-3"
  [[ "$source_control_version" == 12 &&
    "$source_control" == "16d871c7aebb3a34097af219fd5c76a93b3ff1be3af521643dbf3a4d2041c61d" ]] ||
    die "fixed v13 source control-plane identity differs"
  [[ "$source_server" == "ghcr.io/anstosa/weather-server@sha256:d0688756c33875940f67fbb782d9e67a2405a811d2aca40bd155f6ceffa71a74" ]] ||
    die "fixed v13 source server image identity differs"
  [[ "$source_web" == "ghcr.io/anstosa/weather-web@sha256:739d063cd911bcd7c6637082e356889ef60ac7a76caabc737070703fdf83746f" ]] ||
    die "fixed v13 source web image identity differs"
  [[ "$target_version" == 13 ]] ||
    die "fixed v13 source compatibility requires a version 13 target"
  [[ "$target_release" != "$source_release" ]] ||
    die "fixed v13 source compatibility requires a distinct target release"
}

# require the exact source-one identity for the compatibility bridge
require_v14_compatibility_bridge_source_identity() {
  local target_env=$1
  local source_env=$2
  local source_control source_control_version source_release source_server source_web
  local target_release target_version
  source_control=$(env_value "$source_env" WEATHER_CONTROL_PLANE_SHA256)
  source_control_version=$(env_value "$source_env" WEATHER_CONTROL_PLANE_VERSION)
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  source_server=$(env_value "$source_env" WEATHER_SERVER_IMAGE)
  source_web=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)
  target_version=$(env_value "$target_env" WEATHER_CONTROL_PLANE_VERSION)

  [[ "$source_release" == "2026.10.09-1" ]] ||
    die "fixed v14 source compatibility requires release 2026.10.09-1"
  [[ "$source_control_version" == 13 &&
    "$source_control" == "603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba" ]] ||
    die "fixed v14 source control-plane identity differs"
  [[ "$source_server" == "ghcr.io/anstosa/weather-server@sha256:fb140b46d6eaea463ba2d10dc74303eac515135a37c21ddb746cdce744fd23ab" ]] ||
    die "fixed v14 source server image identity differs"
  [[ "$source_web" == "ghcr.io/anstosa/weather-web@sha256:fdcb2d10da4c9ed5ec8651bafa96c9d2b240b66b85db85619b909d6e144e2d7b" ]] ||
    die "fixed v14 source web image identity differs"
  [[ "$target_version" == 15 && "$target_release" == "$v14_compatibility_bridge_release" ]] ||
    die "v14 compatibility bridge target identity differs"
}

# bind the bridge target to installed controls, settings and immutable images
require_v14_compatibility_bridge_release_pair() {
  local target_env=$1
  local source_env=$2
  local key installed_control
  require_v14_compatibility_bridge_source_identity "$target_env" "$source_env"
  installed_control=$(control_plane_digest)
  validate_image_reference "$full_v14_source_server_image"
  validate_image_reference "$full_v14_source_web_image"
  [[ "$(env_value "$target_env" WEATHER_CONTROL_PLANE_SHA256)" == "$installed_control" ]] ||
    die "v14 compatibility bridge target control digest differs"
  [[ "$(env_value "$target_env" WEATHER_SERVER_IMAGE)" == "$full_v14_source_server_image" ]] ||
    die "v14 compatibility bridge target server image differs"
  [[ "$(env_value "$target_env" WEATHER_WEB_IMAGE)" == "$full_v14_source_web_image" ]] ||
    die "v14 compatibility bridge target web image differs"

  # preserve infrastructure and both canary decisions byte-for-byte
  for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH; do
    [[ "$(env_value "$source_env" "$key")" == "$(env_value "$target_env" "$key")" ]] ||
      die "v14 compatibility bridge retained setting differs: $key"
  done
}

# require the exact published bridge identity for the full handoff
require_fixed_v14_source_identity() {
  local target_env=$1
  local source_env=$2
  local installed_control source_control source_control_version source_release source_server source_web
  local target_release target_version
  source_control=$(env_value "$source_env" WEATHER_CONTROL_PLANE_SHA256)
  source_control_version=$(env_value "$source_env" WEATHER_CONTROL_PLANE_VERSION)
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  source_server=$(env_value "$source_env" WEATHER_SERVER_IMAGE)
  source_web=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)
  target_version=$(env_value "$target_env" WEATHER_CONTROL_PLANE_VERSION)
  installed_control=$(control_plane_digest)

  # refuse activation until the two published bridge manifests are frozen
  validate_image_reference "$full_v14_source_server_image"
  validate_image_reference "$full_v14_source_web_image"
  [[ "$source_release" == "$v14_compatibility_bridge_release" ]] ||
    die "full v14 source release differs"
  [[ "$source_control_version" == 15 && "$source_control" == "$installed_control" ]] ||
    die "full v14 source control-plane identity differs"
  [[ "$source_server" == "$full_v14_source_server_image" ]] ||
    die "full v14 source server image identity differs"
  [[ "$source_web" == "$full_v14_source_web_image" ]] ||
    die "full v14 source web image identity differs"
  [[ "$target_version" == 15 && "$target_release" == "$full_v14_release" ]] ||
    die "full v14 target identity differs"
}

# require one exact active release link
require_v14_bridge_active_release() {
  local release=$1
  local active="$state_dir/active.env"
  [[ -L "$active" && "$(readlink "$active")" == "../releases/$release.env" ]] ||
    die "v14 compatibility bridge active release differs"
}

# validate one exact compatibility bridge receipt inode
require_v14_compatibility_bridge_state_file() {
  local path=$1
  local expected_links=$2
  local allowed_fields expected_owner meaningful_lines mode owner links size
  require_file "$path"
  [[ ! -L "$path" ]] || die "v14 compatibility bridge state must not be linked"
  mode=$(stat -c '%a' "$path")
  owner=$(stat -c '%u:%g' "$path")
  links=$(stat -c '%h' "$path")
  size=$(stat -c '%s' "$path")
  expected_owner="$EUID:$(id -g)"
  [[ "$mode" == 600 && "$owner" == "$expected_owner" && "$links" == "$expected_links" &&
    "$size" =~ ^[1-9][0-9]{0,3}$ && "$size" -le 4096 ]] ||
    die "v14 compatibility bridge state metadata differs"
  meaningful_lines=$(grep -cE '^[A-Z][A-Z0-9_]*=' "$path")
  [[ "$meaningful_lines" == 11 ]] || die "v14 compatibility bridge state field count differs"
  allowed_fields='^(WEATHER_V14_BRIDGE_CONTRACT_VERSION|WEATHER_V14_BRIDGE_SOURCE_RELEASE|WEATHER_V14_BRIDGE_TARGET_RELEASE|WEATHER_V14_BRIDGE_TARGET_COMMIT|WEATHER_V14_BRIDGE_CONTROL_SHA256|WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256|WEATHER_V14_BRIDGE_SETTINGS_INODE|WEATHER_V14_BRIDGE_SETTINGS_SHA256|WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256|WEATHER_V14_BRIDGE_TARGET_ENV_SHA256|WEATHER_V14_BRIDGE_PREVIOUS_RELEASE)='
  grep -qEv "$allowed_fields" "$path" && die "v14 compatibility bridge state contains an unknown field"
  [[ "$(env_value "$path" WEATHER_V14_BRIDGE_CONTRACT_VERSION)" == \
      weather-v14-compatibility-bridge/v1 &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_SOURCE_RELEASE)" == "$recurring_previous_release" &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_TARGET_RELEASE)" == "$v14_compatibility_bridge_release" &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_TARGET_COMMIT)" == "$v14_compatibility_bridge_commit" &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_CONTROL_SHA256)" == "$(control_plane_digest)" ]] ||
    die "v14 compatibility bridge state identity differs"
  [[ "$(env_value "$path" WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256)" =~ ^[a-f0-9]{64}$ &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_SETTINGS_INODE)" =~ ^[0-9]+:[0-9]+$ &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_SETTINGS_SHA256)" =~ ^[a-f0-9]{64}$ &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256)" =~ ^[a-f0-9]{64}$ &&
    "$(env_value "$path" WEATHER_V14_BRIDGE_TARGET_ENV_SHA256)" =~ ^[a-f0-9]{64}$ ]] ||
    die "v14 compatibility bridge state proof differs"
  local previous
  previous=$(env_value "$path" WEATHER_V14_BRIDGE_PREVIOUS_RELEASE)
  [[ "$previous" == absent ]] || validate_release "$previous"
}

# finish only the exact fixed hard-link publication window
reconcile_v14_compatibility_bridge_state() {
  local final=$v14_compatibility_bridge_state
  local pending=$v14_compatibility_bridge_pending
  # collapse the only accepted two-link crash state
  if [[ -e "$final" || -L "$final" ]]; then
    if [[ -e "$pending" || -L "$pending" ]]; then
      require_v14_compatibility_bridge_state_file "$final" 2
      require_v14_compatibility_bridge_state_file "$pending" 2
      [[ "$(stat -c '%d:%i' "$final")" == "$(stat -c '%d:%i' "$pending")" ]] ||
        die "v14 compatibility bridge pending receipt differs"
      rm -f -- "$pending"
      sync -f "$state_dir"
    fi
    require_v14_compatibility_bridge_state_file "$final" 1
  fi
}

# validate the durable compatibility bridge receipt
require_v14_compatibility_bridge_state() {
  reconcile_v14_compatibility_bridge_state
  require_v14_compatibility_bridge_state_file "$v14_compatibility_bridge_state" 1
}

# cross-bind the durable receipt to one computed bridge attempt
require_v14_compatibility_bridge_state_inputs() {
  local source_env=$1
  local target_env=$2
  local history_sha256=$3
  local settings_inode=$4
  local settings_sha256=$5
  local previous_release=$6
  require_v14_compatibility_bridge_state
  [[ "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256)" == "$history_sha256" &&
    "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SETTINGS_INODE)" == "$settings_inode" &&
    "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SETTINGS_SHA256)" == "$settings_sha256" &&
    "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256)" == "$(sha256sum "$source_env" | awk '{print $1}')" &&
    "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_TARGET_ENV_SHA256)" == "$(sha256sum "$target_env" | awk '{print $1}')" &&
    "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_PREVIOUS_RELEASE)" == "$previous_release" ]] ||
    die "v14 compatibility bridge state inputs differ"
}

# publish one create-once bridge receipt before runtime mutation
publish_v14_compatibility_bridge_state() {
  local source_env=$1
  local target_env=$2
  local history_sha256=$3
  local settings_inode=$4
  local settings_sha256=$5
  local previous_release=$6
  local temporary
  [[ "$history_sha256" =~ ^[a-f0-9]{64}$ && "$settings_inode" =~ ^[0-9]+:[0-9]+$ &&
    "$settings_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "v14 compatibility bridge state inputs are invalid"
  [[ "$previous_release" == absent ]] || validate_release "$previous_release"
  reconcile_v14_compatibility_bridge_state
  # finish an exact durable pending receipt from a pre-link crash
  if [[ ! -e "$v14_compatibility_bridge_state" && ! -L "$v14_compatibility_bridge_state" &&
    ( -e "$v14_compatibility_bridge_pending" || -L "$v14_compatibility_bridge_pending" ) ]]; then
    require_v14_compatibility_bridge_state_file "$v14_compatibility_bridge_pending" 1
    ln "$v14_compatibility_bridge_pending" "$v14_compatibility_bridge_state" ||
      die "v14 compatibility bridge state publication collided"
    sync -f "$state_dir"
    rm -f -- "$v14_compatibility_bridge_pending"
    sync -f "$state_dir"
    require_v14_compatibility_bridge_state_inputs "$source_env" "$target_env" \
      "$history_sha256" "$settings_inode" "$settings_sha256" "$previous_release"
    return
  fi
  [[ ! -e "$v14_compatibility_bridge_state" && ! -L "$v14_compatibility_bridge_state" ]] ||
    die "v14 compatibility bridge state already exists"
  [[ ! -e "$v14_compatibility_bridge_pending" && ! -L "$v14_compatibility_bridge_pending" ]] ||
    die "v14 compatibility bridge pending state already exists"
  temporary=$(mktemp "$state_dir/.v14-compatibility-bridge.XXXXXX")
  trap 'rm -f -- "$temporary"' RETURN
  umask 077
  printf '%s\n' \
    'WEATHER_V14_BRIDGE_CONTRACT_VERSION=weather-v14-compatibility-bridge/v1' \
    "WEATHER_V14_BRIDGE_SOURCE_RELEASE=$recurring_previous_release" \
    "WEATHER_V14_BRIDGE_TARGET_RELEASE=$v14_compatibility_bridge_release" \
    "WEATHER_V14_BRIDGE_TARGET_COMMIT=$v14_compatibility_bridge_commit" \
    "WEATHER_V14_BRIDGE_CONTROL_SHA256=$(control_plane_digest)" \
    "WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256=$history_sha256" \
    "WEATHER_V14_BRIDGE_SETTINGS_INODE=$settings_inode" \
    "WEATHER_V14_BRIDGE_SETTINGS_SHA256=$settings_sha256" \
    "WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256=$(sha256sum "$source_env" | awk '{print $1}')" \
    "WEATHER_V14_BRIDGE_TARGET_ENV_SHA256=$(sha256sum "$target_env" | awk '{print $1}')" \
    "WEATHER_V14_BRIDGE_PREVIOUS_RELEASE=$previous_release" >"$temporary"
  chmod 600 "$temporary"
  chown "$EUID:$(id -g)" "$temporary"
  sync -f "$temporary"
  mv -T -n "$temporary" "$v14_compatibility_bridge_pending"
  [[ ! -e "$temporary" && ! -L "$temporary" ]] ||
    die "v14 compatibility bridge pending state publication collided"
  sync -f "$state_dir"
  ln "$v14_compatibility_bridge_pending" "$v14_compatibility_bridge_state" ||
    die "v14 compatibility bridge state publication collided"
  sync -f "$state_dir"
  rm -f -- "$v14_compatibility_bridge_pending"
  sync -f "$state_dir"
  trap - RETURN
  require_v14_compatibility_bridge_state_inputs "$source_env" "$target_env" \
    "$history_sha256" "$settings_inode" "$settings_sha256" "$previous_release"
}

# write the fixed pre-epoch compose override
write_v14_compatibility_compose_override() {
  local override=$1
  cat >"$override" <<'YAML'
services:
  api:
    volumes: !reset []
  worker:
    volumes: !reset []
YAML
  chmod 600 "$override"
}

# prove one pre-epoch compose view contains no future-state mounts
require_v14_compatibility_compose_override() {
  local environment=$1
  local override=$2
  # prove the bridge does not depend on uninitialized maintenance authority files
  docker compose --project-name "${WEATHER_COMPOSE_PROJECT_NAME:-weather}" \
    --env-file "$environment" -f "$compose_file" -f "$override" config --format json | \
    node --input-type=module -e '
import { readFileSync } from "node:fs";
const configuration = JSON.parse(readFileSync(0, "utf8"));
// reject any inherited or future API and worker filesystem authority
if (configuration.services?.api?.volumes !== undefined ||
  configuration.services?.worker?.volumes !== undefined) {
  throw new Error("v14 compatibility bridge retained future-state mounts");
}
'
}

# run one compose operation through the fixed pre-epoch view
v14_compatibility_compose() {
  local environment=$1
  local override=$2
  shift 2
  docker compose --project-name "${WEATHER_COMPOSE_PROJECT_NAME:-weather}" \
    --env-file "$environment" -f "$compose_file" -f "$override" "$@"
}

# start only runtime containers without schema, ACL or future-state mounts
start_v14_compatibility_bridge_runtime() (
  local environment=$1
  local override
  override=$(mktemp "$state_dir/.v14-compatibility-compose.XXXXXX.yaml") ||
    die "v14 compatibility bridge override could not be created"
  trap 'rm -f -- "$override"' EXIT
  write_v14_compatibility_compose_override "$override"
  require_v14_compatibility_compose_override "$environment" "$override"
  v14_compatibility_compose "$environment" "$override" \
    up -d --no-deps --remove-orphans --wait api worker web cloudflared
)

# restore source-one runtime and markers after an interrupted bridge
recover_v14_compatibility_bridge() {
  local source_env target_env previous history settings_inode settings_sha256 actual_settings
  local recovery_status=0
  [[ -e "$v14_compatibility_bridge_state" || -L "$v14_compatibility_bridge_state" ]] || return 0
  require_v14_compatibility_bridge_state || return 1
  source_env=$(release_env "$recurring_previous_release")
  target_env=$(release_env "$v14_compatibility_bridge_release")
  validate_release_env "$source_env" "$recurring_previous_release" || return 1
  validate_release_env "$target_env" "$v14_compatibility_bridge_release" || return 1
  [[ "$(sha256sum "$source_env" | awk '{print $1}')" == \
      "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256)" &&
    "$(sha256sum "$target_env" | awk '{print $1}')" == \
      "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_TARGET_ENV_SHA256)" ]] ||
    die "v14 compatibility bridge recovery environment differs"
  history=$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256)
  settings_inode=$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SETTINGS_INODE)
  settings_sha256=$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SETTINGS_SHA256)
  actual_settings=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    inert-v14-settings-snapshot) || return 1
  [[ "$actual_settings" == "$settings_inode $settings_sha256" ]] ||
    die "v14 compatibility bridge recovery settings differ"

  # restore exact source images without touching ACLs or migrations
  start_v14_compatibility_bridge_runtime "$source_env" || recovery_status=1
  require_v14_compatibility_bridge_migration_ledger \
    "$source_env" "$(env_value "$source_env" WEATHER_DATABASE_NAME)" || recovery_status=1
  [[ "$(migration_history_sha256 "$source_env" \
      "$(env_value "$source_env" WEATHER_DATABASE_NAME)")" == "$history" ]] || recovery_status=1
  ((recovery_status == 0)) || return 1
  previous=$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_PREVIOUS_RELEASE)
  # restore the original rollback marker before source becomes current again
  if [[ "$previous" == absent ]]; then
    rm -f -- "$state_dir/previous-release" || return 1
  else
    write_private_state "$state_dir/previous-release" "$previous" || return 1
  fi
  write_private_state "$state_dir/schema-release" "$recurring_previous_release" || return 1
  write_active_symlink "$recurring_previous_release" || return 1
  write_private_state "$state_dir/current-release" "$recurring_previous_release" || return 1
  rm -f -- "$v14_compatibility_bridge_state"
}

# require the immutable bridge receipt without assuming the current schema state
require_v14_compatibility_bridge_receipt() {
  local source_env target_env
  require_v14_compatibility_bridge_state
  source_env=$(release_env "$recurring_previous_release")
  target_env=$(release_env "$v14_compatibility_bridge_release")
  validate_release_env "$source_env" "$recurring_previous_release"
  validate_release_env "$target_env" "$v14_compatibility_bridge_release"
  require_v14_compatibility_bridge_release_pair "$target_env" "$source_env"
  [[ "$(sha256sum "$source_env" | awk '{print $1}')" == \
      "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256)" &&
    "$(sha256sum "$target_env" | awk '{print $1}')" == \
      "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_TARGET_ENV_SHA256)" ]] ||
    die "v14 compatibility bridge receipt environments differ"
}

# require the original operator settings bound by the bridge receipt
require_v14_compatibility_bridge_settings() {
  local settings
  settings=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    inert-v14-settings-snapshot)
  [[ "$settings" == \
      "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SETTINGS_INODE) $(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SETTINGS_SHA256)" ]] ||
    die "v14 compatibility bridge settings differ"
}

# require the complete committed bridge before the full migration handoff
require_committed_v14_compatibility_bridge() {
  local target_env history
  require_v14_compatibility_bridge_receipt
  [[ "$(read_release_state "$state_dir/current-release")" == "$v14_compatibility_bridge_release" &&
    "$(read_release_state "$state_dir/schema-release")" == "$v14_compatibility_bridge_release" ]] ||
    die "full v14 requires committed bridge runtime and schema markers"
  require_v14_bridge_active_release "$v14_compatibility_bridge_release"
  target_env=$(release_env "$v14_compatibility_bridge_release")
  require_v14_compatibility_bridge_migration_ledger \
    "$target_env" "$(env_value "$target_env" WEATHER_DATABASE_NAME)"
  history=$(migration_history_sha256 \
    "$target_env" "$(env_value "$target_env" WEATHER_DATABASE_NAME)")
  [[ "$history" == \
      "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256)" ]] ||
    die "committed v14 compatibility bridge ledger differs"
  require_v14_compatibility_bridge_settings
}

# require source-two authorization for the exact complete full schema
require_v14_source2_full_schema_authority() {
  local environment=$1
  local authorization actual_history expected_history database
  require_v14_compatibility_bridge_receipt
  validate_release_env "$environment" "$v14_compatibility_bridge_release"
  authorization=$(migration_authorization "$full_v14_release")
  validate_migration_authorization "$authorization" \
    "$v14_compatibility_bridge_release" "$full_v14_release"
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  actual_history=$(migration_history_sha256 "$environment" "$database")
  expected_history=$(env_value \
    "$authorization" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)
  [[ "$actual_history" == "$expected_history" ]] ||
    die "v14 source-two full-schema history differs"
  require_fixed_v14_migration_ledger "$environment" "$database"
  require_v14_compatibility_bridge_settings
}

# admit the full transaction from pristine or authorized compensated source two
require_v14_full_source_authority() {
  local environment=$1
  local schema
  [[ "$(read_release_state "$state_dir/current-release")" == \
    "$v14_compatibility_bridge_release" ]] ||
    die "full v14 requires the exact compatibility bridge runtime"
  require_v14_bridge_active_release "$v14_compatibility_bridge_release"
  schema=$(read_release_state "$state_dir/schema-release")
  # distinguish pristine schema 18 from an authorized full-schema retry
  if [[ "$schema" == "$v14_compatibility_bridge_release" ]]; then
    require_committed_v14_compatibility_bridge
  elif [[ "$schema" == "$full_v14_release" ]]; then
    require_v14_source2_full_schema_authority "$environment"
  else
    die "full v14 source schema marker differs"
  fi
}

# require the root current authority before accepting a committed full release
require_full_v14_current_authority() {
  local environment=$1
  local catalog="$state_dir/adjustment-candidate-catalog.json"
  local catalog_sha256 settings settings_identity settings_sha256
  require_file "$catalog"
  catalog_sha256=$(sha256sum "$catalog" | awk '{print $1}')
  settings=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    inert-v14-settings-snapshot)
  settings_identity=${settings%% *}
  settings_sha256=${settings#* }
  # validate live settings independently from the retained commit-time identity
  [[ "$catalog_sha256" =~ ^[a-f0-9]{64}$ && "$settings_identity" =~ ^[0-9]+:[0-9]+$ &&
    "$settings_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "full v14 current authority inputs are invalid"
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" verify-inert-v14-current \
    "$full_v14_release" "$(env_value "$environment" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$environment" WEATHER_WEB_IMAGE)" "$catalog_sha256" >/dev/null ||
    die "full v14 current authority is unavailable"
}

# verify the unchanged source ledger before and after the compatibility bridge
require_v14_compatibility_bridge_migration_ledger() {
  local environment=$1
  local database=$2
  WEATHER_ENV_FILE=$environment compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database" -tAc \
    'SELECT row_to_json(manifest) FROM adjustment_evaluation_export_manifest_v1 manifest' | \
    node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
      verify-maintenance-ledger-v13-compatibility ||
    die "v14 compatibility bridge requires the exact complete 0018 ledger"
}

# verify all migration names and bytes before publishing v14 compatibility authority
require_fixed_v14_migration_ledger() {
  local environment=$1
  local database=$2
  WEATHER_ENV_FILE=$environment compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database" -tAc \
    'SELECT row_to_json(manifest) FROM adjustment_evaluation_export_manifest_v1 manifest' | \
    node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" verify-maintenance-ledger-v14-rolling ||
    die "fixed v14 compatibility fixture lacks the exact complete 0021 ledger"
}

# create or verify only the fixed dedicated rain target sources
initialize_adjustment_rain_fixed_gauge_target_sources_v1() (
  local environment=$1
  local database
  [[ "$EUID" == 0 ]] || die "rain target source initialization requires root"
  validate_release_env "$environment" "$(env_value "$environment" WEATHER_RELEASE)"
  require_control_plane_compatibility "$environment"
  [[ "$(env_value "$environment" WEATHER_CONTROL_PLANE_VERSION)" == 15 ]] ||
    die "rain target source initialization requires v15"
  require_deployment_secrets
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  require_fixed_v14_migration_ledger "$environment" "$database"
  # keep the existing owner secret inside the database container process environment
  # shellcheck disable=SC2016
  node --max-old-space-size=48 --max-semi-space-size=1 \
    "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    rain-fixed-gauge-target-sources-sql-v1 | \
    WEATHER_ENV_FILE=$environment compose exec -T postgres \
      sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
      adjustment-rain-target-sources "$database" | \
    node --max-old-space-size=48 --max-semi-space-size=1 \
      "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
      project-rain-fixed-gauge-target-sources-v1
)

# retry the fixed source initialization only against the current v14 release
initialize_current_adjustment_rain_fixed_gauge_target_sources_v1() (
  local current environment
  [[ "$EUID" == 0 ]] || die "rain target source initialization requires root"
  acquire_release_transaction_lock
  current=$(read_release_state "$state_dir/current-release")
  environment=$(release_env "$current")
  initialize_adjustment_rain_fixed_gauge_target_sources_v1 "$environment"
)

# initialize only the create-once schedule independently derived from the root epoch
initialize_adjustment_registration_schedule_v3() (
  local bootstrap_sha256=$1
  local current environment database temporary
  [[ "$EUID" == 0 && "$bootstrap_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "registration schedule initialization requires root and one hash"
  acquire_release_transaction_lock
  current=$(read_release_state "$state_dir/current-release")
  environment=$(release_env "$current")
  validate_release_env "$environment" "$current"
  require_control_plane_compatibility "$environment"
  [[ "$(env_value "$environment" WEATHER_CONTROL_PLANE_VERSION)" == 15 ]] ||
    die "registration schedule requires the current v14 control plane"
  require_deployment_secrets
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  require_fixed_v14_migration_ledger "$environment" "$database"
  temporary=$(mktemp "$state_dir/.registration-schedule.XXXXXX.sql")
  chmod 600 "$temporary"
  # remove only this unpublished bounded SQL inode on every exit
  trap 'rm -f -- "$temporary"' EXIT
  node --max-old-space-size=48 --max-semi-space-size=1 \
    "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    registration-schedule-initialize-sql-v3 "$bootstrap_sha256" >"$temporary"
  # keep the existing owner secret inside the database container process environment
  # shellcheck disable=SC2016
  WEATHER_ENV_FILE=$environment compose exec -T postgres \
    sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
    adjustment-registration-schedule "$database" <"$temporary" | \
    node --max-old-space-size=48 --max-semi-space-size=1 \
      "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
      verify-registration-schedule-initialization-v3 "$bootstrap_sha256"
)

# read only closed rolling registrations and reconciled predecessor identities
read_adjustment_registration_lifecycle_status_v4() (
  local current environment database
  [[ "$EUID" == 0 ]] || die "registration lifecycle status requires root"
  acquire_release_transaction_lock
  current=$(read_release_state "$state_dir/current-release")
  environment=$(release_env "$current")
  validate_release_env "$environment" "$current"
  require_control_plane_compatibility "$environment"
  [[ "$(env_value "$environment" WEATHER_CONTROL_PLANE_VERSION)" == 15 ]] ||
    die "registration lifecycle status requires v15"
  require_deployment_secrets
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  require_fixed_v14_migration_ledger "$environment" "$database"
  # keep all SQL and schema identities inside the fixed reviewed owner boundary
  # shellcheck disable=SC2016
  node --max-old-space-size=48 --max-semi-space-size=1 \
    "$deploy_dir/scripts/adjustment-evaluation-package.mjs" registration-lifecycle-status-sql-v4 | \
    WEATHER_ENV_FILE=$environment compose exec -T postgres \
      sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
      adjustment-registration-lifecycle "$database" | \
    node --max-old-space-size=48 --max-semi-space-size=1 \
      "$deploy_dir/scripts/adjustment-evaluation-package.mjs" project-registration-lifecycle-status-v4
)

# execute only a canonical authenticated owner operation under the deployment fence
run_adjustment_owner_operation_v3() (
  local kind=$1 identity=$2 current environment database result_kind
  [[ "$EUID" == 0 ]] || die "adjustment owner operation requires root"
  [[ "$kind" =~ ^(access|terminal|retire|unsupported-terminal|unsupported-retire)$ &&
    "$identity" =~ ^[a-f0-9]{64}$ ]] || die "adjustment owner operation is invalid"
  # reuse only the closed terminal response grammar after disjoint request authorization
  case "$kind" in
    unsupported-terminal) result_kind=terminal ;;
    unsupported-retire) result_kind=retire ;;
    *) result_kind=$kind ;;
  esac
  acquire_release_transaction_lock
  current=$(read_release_state "$state_dir/current-release")
  environment=$(release_env "$current")
  validate_release_env "$environment" "$current"
  require_control_plane_compatibility "$environment"
  [[ "$(env_value "$environment" WEATHER_CONTROL_PLANE_VERSION)" == 15 ]] || die "adjustment owner operation requires v15"
  require_deployment_secrets
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  require_fixed_v14_migration_ledger "$environment" "$database"
  # stdin remains the bounded request and no password enters command arguments
  # shellcheck disable=SC2016
  node --max-old-space-size=48 --max-semi-space-size=1 "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    owner-operation-sql-v3 "$kind" "$identity" | \
    WEATHER_ENV_FILE=$environment compose exec -T postgres \
      sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
      adjustment-owner-operation "$database" | \
    node --max-old-space-size=48 --max-semi-space-size=1 "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
      owner-operation-result-v3 "$result_kind"
)

# finalize only compact rows covered by the current authenticated custody proof
finalize_adjustment_shadow_metadata_custody_v1() (
  local proof=$1 current environment database preparation consumed
  [[ "$EUID" == 0 ]] || die "shadow metadata custody finalization requires root"
  [[ "$proof" =~ ^[a-f0-9]{64}$ ]] || die "shadow metadata custody identity is invalid"
  require_command setpriv
  acquire_release_transaction_lock
  current=$(read_release_state "$state_dir/current-release")
  environment=$(release_env "$current")
  validate_release_env "$environment" "$current"
  require_control_plane_compatibility "$environment"
  [[ "$(env_value "$environment" WEATHER_CONTROL_PLANE_VERSION)" == 15 ]] ||
    die "shadow metadata custody finalization requires v15"
  require_deployment_secrets
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  validate_database_name "$database"
  require_fixed_v14_migration_ledger "$environment" "$database"
  # preserve the actual consumed receipt when the SSH response was lost
  if consumed=$(setpriv --reuid=10002 --regid=10002 --clear-groups \
    node --max-old-space-size=48 --max-semi-space-size=1 \
      "$deploy_dir/scripts/adjustment-evidence-store.mjs" shadow-metadata-custody-consumed-v1 "$proof"); then
    # null means that the current proof still needs its owner transaction
    if [[ "$consumed" != null ]]; then
      printf '%s\n' "$consumed"
      return
    fi
  fi
  preparation=$(setpriv --reuid=10002 --regid=10002 --clear-groups \
    node --max-old-space-size=48 --max-semi-space-size=1 \
      "$deploy_dir/scripts/adjustment-evaluation-package.mjs" shadow-metadata-preparation-identity-v1 "$proof")
  # persist native manifest and successor arguments before deleting any hot row
  if [[ "$preparation" == unprepared ]]; then
    # shellcheck disable=SC2016
    setpriv --reuid=10002 --regid=10002 --clear-groups \
      node --max-old-space-size=48 --max-semi-space-size=1 \
        "$deploy_dir/scripts/adjustment-evaluation-package.mjs" shadow-metadata-preparation-sql-v1 "$proof" | \
      WEATHER_ENV_FILE=$environment compose exec -T postgres \
        sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
        adjustment-shadow-metadata "$database" | \
      setpriv --reuid=10002 --regid=10002 --clear-groups \
        node --max-old-space-size=48 --max-semi-space-size=1 \
          "$deploy_dir/scripts/adjustment-evaluation-package.mjs" prepare-shadow-metadata-custody-v1 "$proof" >/dev/null
    preparation=$(setpriv --reuid=10002 --regid=10002 --clear-groups \
      node --max-old-space-size=48 --max-semi-space-size=1 \
        "$deploy_dir/scripts/adjustment-evaluation-package.mjs" shadow-metadata-preparation-identity-v1 "$proof")
  fi
  [[ "$preparation" =~ ^[a-f0-9]{64}$ ]] || die "shadow metadata custody preparation is unavailable"
  # the native function verifies the exact prepared generation or its byte-identical retry
  # shellcheck disable=SC2016
  setpriv --reuid=10002 --regid=10002 --clear-groups \
    node --max-old-space-size=48 --max-semi-space-size=1 \
      "$deploy_dir/scripts/adjustment-evaluation-package.mjs" shadow-metadata-finalization-sql-v1 "$preparation" | \
    WEATHER_ENV_FILE=$environment compose exec -T postgres \
      sh -eu -c 'PGPASSWORD=$(cat /run/secrets/weather_owner_password); export PGPASSWORD; exec psql --no-password --no-psqlrc --quiet --tuples-only --no-align --set=ON_ERROR_STOP=1 --host 127.0.0.1 --username weather_owner --dbname "$1"' \
      adjustment-shadow-metadata "$database" | \
    setpriv --reuid=10002 --regid=10002 --clear-groups \
      node --max-old-space-size=48 --max-semi-space-size=1 \
        "$deploy_dir/scripts/adjustment-evaluation-package.mjs" consume-shadow-metadata-custody-v1 "$preparation"
)

# require one caller-created private publication inode
require_fixed_v13_authorization_target() {
  local path=$1
  local owner mode links size
  require_canonical_descendant "$path" "$releases_dir" "migration authorization"
  require_file "$path"
  [[ -f "$path" && ! -L "$path" ]] ||
    die "fixed v13 source compatibility authorization must be a regular file"
  read -r owner mode links size < <(stat --format='%u %a %h %s' "$path")
  [[ "$owner" == "$EUID" && "$mode" == 600 && "$links" == 1 && "$size" == 0 ]] ||
    die "fixed v13 source compatibility authorization must be a private empty single-link file"
}

# retain the exact immutable v13 bridge contract
verify_fixed_v13_source_compatibility() {
  verify_bounded_maintenance_source_compatibility "$1" "$2" "$3" v13
}

# prove the published compatibility bridge only for its full v14 successor
verify_fixed_v14_source_compatibility() {
  verify_bounded_maintenance_source_compatibility "$1" "$2" "$3" v14
}

# share the bounded fixture without widening either source identity
verify_bounded_maintenance_source_compatibility() (
  local target_env=$1
  local source_env=$2
  local authorization_path=$3
  local scope=$4
  local candidate api_container unproven_api_container provider_container
  local provider_image provider_network source_release target_release history_sha256
  local compatibility_compose_override=
  local ledger_state non_compatibility_source_ids compatibility_source_count
  local before_successes after_successes before_forecasts after_forecasts
  local start_free_bytes start_free_inodes start_wal_lsn
  local wal_bytes current_free_bytes current_free_inodes unproven_status
  local fixture_budget_bytes=$((64 * 1024 * 1024))
  local fixture_budget_inodes=4096
  local peak_fixture_bytes=0
  local peak_filesystem_bytes=0
  local peak_filesystem_inodes=0
  local candidate_created=false
  local api_started=false
  local unproven_api_started=false
  local provider_started=false
  [[ "$scope" == v13 || "$scope" == v14 ]] || die "unsupported bounded maintenance compatibility scope"
  candidate="weather_${scope}_compat_$(date -u +%Y%m%d%H%M%S)_$$"
  api_container="${candidate}_api"
  unproven_api_container="${candidate}_api_unproven"
  provider_container="${candidate}_provider"
  provider_image=$(env_value "$target_env" WEATHER_SERVER_IMAGE)
  provider_network="${WEATHER_COMPOSE_PROJECT_NAME:-weather}_provider_egress"
  source_release=$(env_value "$source_env" WEATHER_RELEASE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)

  validate_release_env "$source_env" "$source_release"
  validate_release_env "$target_env" "$target_release"
  # enforce the corresponding fixed source instead of a caller-selected predecessor
  if [[ "$scope" == v13 ]]; then
    require_fixed_v13_source_identity "$target_env" "$source_env"
  else
    require_fixed_v14_source_identity "$target_env" "$source_env"
  fi
  require_fixed_v13_authorization_target "$authorization_path"
  # strip unavailable future authority only from v14 source runtime probes
  if [[ "$scope" == v14 ]]; then
    compatibility_compose_override=$(mktemp \
      "$state_dir/.v14-compatibility-probe.XXXXXX.yaml")
    write_v14_compatibility_compose_override "$compatibility_compose_override"
    require_v14_compatibility_compose_override \
      "$source_env" "$compatibility_compose_override"
  fi

  # select the ordinary or pre-epoch compose view for source runtime probes
  source_compatibility_compose() {
    # keep the existing v13 fixture byte-for-byte unchanged
    if [[ "$scope" == v14 ]]; then
      v14_compatibility_compose "$source_env" "$compatibility_compose_override" "$@"
    else
      WEATHER_ENV_FILE=$source_env compose "$@"
    fi
  }

  # sample free filesystem capacity without prospective cleanup credit
  read -r start_free_bytes start_free_inodes < <(
    stat --file-system --format='%a %S %d' /var/lib/weather |
      awk '{ printf "%.0f %s\n", $1 * $2, $3 }'
  )
  [[ "$start_free_bytes" =~ ^[0-9]+$ && "$start_free_inodes" =~ ^[0-9]+$ ]] ||
    die "fixed v13 compatibility capacity sample is invalid"
  if ((start_free_bytes < image_pull_required_free_bytes + fixture_budget_bytes ||
    start_free_inodes < image_pull_required_free_inodes + fixture_budget_inodes)); then
    die "fixed v13 compatibility fixture lacks its bounded reservation"
  fi

  # clean every disposable bridge resource
  # shellcheck disable=SC2317,SC2329
  cleanup_fixed_v13_compatibility() {
    local status=$?

    # remove a started API probe
    if [[ "$api_started" == true ]]; then
      docker rm --force "$api_container" >/dev/null 2>&1 || status=1
    fi

    # remove a started rejection probe
    if [[ "$unproven_api_started" == true ]]; then
      docker rm --force "$unproven_api_container" >/dev/null 2>&1 || status=1
    fi

    # remove a started provider stub
    if [[ "$provider_started" == true ]]; then
      docker rm --force "$provider_container" >/dev/null 2>&1 || status=1
    fi

    # drop the isolated fixture last
    if [[ "$candidate_created" == true ]]; then
      WEATHER_ENV_FILE=$source_env compose exec -T postgres \
        dropdb --username postgres --if-exists "$candidate" >/dev/null || status=1
    fi

    # remove only this invocation's private pre-epoch override
    if [[ -n "$compatibility_compose_override" ]]; then
      rm -f -- "$compatibility_compose_override" || status=1
    fi

    trap - EXIT
    exit "$status"
  }
  trap cleanup_fixed_v13_compatibility EXIT
  trap 'exit 130' HUP INT TERM

  start_wal_lsn=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres \
      --dbname "$(env_value "$source_env" WEATHER_DATABASE_NAME)" \
      --tuples-only --no-align --command "SELECT pg_current_wal_lsn()")
  [[ "$start_wal_lsn" =~ ^[0-9A-F]+/[0-9A-F]+$ ]] ||
    die "fixed v13 compatibility WAL identity is invalid"

  # enforce the fixture ceiling after each bounded phase
  measure_fixed_v13_compatibility() {
    local phase=$1
    local database_name=${2:-}
    local fixture_sample=0
    local filesystem_sample filesystem_inode_sample

    # include the live fixture while it exists
    if [[ -n "$database_name" ]]; then
      fixture_sample=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
        psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
          --tuples-only --no-align --command "SELECT pg_database_size(current_database())")
    fi
    wal_bytes=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres \
        --dbname "$(env_value "$source_env" WEATHER_DATABASE_NAME)" \
        --tuples-only --no-align --command "SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), '$start_wal_lsn'::pg_lsn)::bigint")
    read -r current_free_bytes current_free_inodes < <(
      stat --file-system --format='%a %S %d' /var/lib/weather |
        awk '{ printf "%.0f %s\n", $1 * $2, $3 }'
    )
    [[ "$fixture_sample" =~ ^[0-9]+$ && "$wal_bytes" =~ ^[0-9]+$ &&
      "$current_free_bytes" =~ ^[0-9]+$ && "$current_free_inodes" =~ ^[0-9]+$ ]] ||
      die "fixed v13 compatibility usage sample is invalid during $phase"
    filesystem_sample=$((start_free_bytes - current_free_bytes))
    filesystem_inode_sample=$((start_free_inodes - current_free_inodes))

    # ignore only capacity that another process returned
    if ((filesystem_sample < 0)); then
      filesystem_sample=0
    fi
    if ((filesystem_inode_sample < 0)); then
      filesystem_inode_sample=0
    fi

    # retain each observed high-water mark
    if ((fixture_sample > peak_fixture_bytes)); then
      peak_fixture_bytes=$fixture_sample
    fi
    if ((filesystem_sample > peak_filesystem_bytes)); then
      peak_filesystem_bytes=$filesystem_sample
    fi
    if ((filesystem_inode_sample > peak_filesystem_inodes)); then
      peak_filesystem_inodes=$filesystem_inode_sample
    fi

    # stop before the next phase when any measured envelope is exhausted
    if ((peak_fixture_bytes + wal_bytes > fixture_budget_bytes ||
      peak_filesystem_bytes > fixture_budget_bytes ||
      peak_filesystem_inodes > fixture_budget_inodes ||
      current_free_bytes < image_pull_required_free_bytes ||
      current_free_inodes < image_pull_required_free_inodes)); then
      die "fixed v13 compatibility fixture exceeded its bounded reservation during $phase"
    fi
  }

  printf 'Creating bounded fixed-v13 compatibility database %s...\n' "$candidate"
  WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    createdb --username postgres --owner weather_owner --template template0 "$candidate"
  candidate_created=true
  measure_fixed_v13_compatibility create "$candidate"

  # bootstrap only public target configuration into the empty fixture
  WEATHER_ENV_FILE=$target_env compose run --rm --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" migration
  apply_runtime_database_acl "$source_env" "$candidate"
  verify_runtime_database_acl "$source_env" "$candidate"
  history_sha256=$(migration_history_sha256 "$source_env" "$candidate")
  ledger_state=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
      --tuples-only --no-align --command "SELECT count(*)::text || ':' || count(*) FILTER (WHERE name = '0018_adjustment_maintenance_v2.sql')::text || ':' || COALESCE(max(checksum) FILTER (WHERE name = '0018_adjustment_maintenance_v2.sql'), '') FROM schema_migrations")
  # the historical bridge retains its exact eighteen-migration boundary
  if [[ "$scope" == v13 ]]; then
    [[ "$ledger_state" == "18:1:c13d2c2c39096887712ae97f0b863f8a7ab52074ca320575f8c4c1f9b72a50a3" ]] ||
      die "fixed v13 compatibility fixture lacks the exact complete 0018 ledger"
  else
    require_fixed_v14_migration_ledger "$source_env" "$candidate"
  fi
  measure_fixed_v13_compatibility schema "$candidate"

  non_compatibility_source_ids=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
      --tuples-only --no-align --command "SELECT COALESCE(string_agg(s.id::text, ',' ORDER BY s.id), '') FROM sources s JOIN providers p ON p.id = s.provider_id WHERE s.active AND p.provider_key <> 'open-meteo'")
  [[ -z "$non_compatibility_source_ids" ||
    "$non_compatibility_source_ids" =~ ^[0-9]+(,[0-9]+)*$ ]] ||
    die "fixed v13 non-compatibility source identity is invalid"
  compatibility_source_count=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
      --tuples-only --no-align --command "SELECT count(*) FROM sources s JOIN providers p ON p.id = s.provider_id WHERE s.active AND p.provider_key = 'open-meteo'")
  [[ "$compatibility_source_count" =~ ^[1-9][0-9]*$ ]] ||
    die "fixed v13 Open-Meteo compatibility source is missing"

  # isolate the exact source worker to the deterministic provider
  if [[ -n "$non_compatibility_source_ids" ]]; then
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
        --command "UPDATE sources SET active = false WHERE id IN ($non_compatibility_source_ids)"
  fi
  WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
      --command "UPDATE ingestion_checkpoints SET last_committed_at = TIMESTAMPTZ '1970-01-01 00:00:00+00'; UPDATE sources SET cadence_seconds = 60 WHERE active"
  measure_fixed_v13_compatibility configuration "$candidate"

  docker run --detach --rm --name "$provider_container" \
    --network "$provider_network" --network-alias "$provider_container" \
    --env WEATHER_COMPATIBILITY_DYNAMIC_FORECAST=1 \
    "$provider_image" node deploy/scripts/compatibility-provider.mjs >/dev/null
  provider_started=true

  # wait for the deterministic provider
  for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
    # stop after its first successful readiness response
    if docker exec "$provider_container" node -e \
      "fetch('http://127.0.0.1:3002/health').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"; then
      break
    fi
    sleep 1
  done
  ((_attempt < 30)) || die "fixed v13 compatibility provider did not become ready"

  before_successes=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM ingestion_runs WHERE state = 'succeeded'")
  before_forecasts=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM weather_records wr JOIN sources s ON s.id = wr.source_id WHERE s.source_key = 'open-meteo-forecast-v4'")
  source_compatibility_compose run --rm --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" \
    --env WEATHER_MIGRATION_AUTHORIZATION_RELEASE="$source_release" \
    --env WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256="$history_sha256" \
    --env WEATHER_OPEN_METEO_COMPATIBILITY_ORIGIN="http://$provider_container:3002" \
    worker node apps/worker/dist/worker.js --once
  after_successes=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM ingestion_runs WHERE state = 'succeeded'")
  after_forecasts=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM weather_records wr JOIN sources s ON s.id = wr.source_id WHERE s.source_key = 'open-meteo-forecast-v4'")
  [[ "$before_successes" =~ ^[0-9]+$ && "$after_successes" =~ ^[0-9]+$ &&
    "$before_forecasts" =~ ^[0-9]+$ && "$after_forecasts" =~ ^[0-9]+$ ]] ||
    die "fixed v13 source worker returned an invalid proof count"
  if ((after_successes <= before_successes || after_forecasts <= before_forecasts)); then
    die "fixed v13 source worker did not persist the deterministic forecast"
  fi
  measure_fixed_v13_compatibility worker "$candidate"

  # require the source worker to reject the unproven trailing ledger
  if source_compatibility_compose run --rm --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" \
    worker node apps/worker/dist/health.js >/dev/null 2>&1; then
    die "fixed v13 source worker accepted unproven migration history"
  fi

  source_compatibility_compose run --detach \
    --name "$unproven_api_container" --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" \
    --env WEATHER_MIGRATION_AUTHORIZATION_RELEASE="$source_release" \
    --env WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256="$(printf '%064d' 0)" \
    api node apps/api/dist/main.js >/dev/null
  unproven_api_started=true

  # require the source API to reject a wrong ledger digest
  for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
    # inspect the first reachable response
    if unproven_status=$(docker exec "$unproven_api_container" node -e \
      "fetch('http://127.0.0.1:3001/api/v1/health').then(r=>console.log(r.status)).catch(()=>process.exit(1))" 2>/dev/null); then
      [[ "$unproven_status" == 503 ]] ||
        die "fixed v13 source API accepted invalid migration authorization"
      break
    fi
    sleep 1
  done
  ((_attempt < 30)) || die "fixed v13 source API did not reject invalid authorization"
  docker rm --force "$unproven_api_container" >/dev/null
  unproven_api_started=false
  measure_fixed_v13_compatibility rejection "$candidate"

  source_compatibility_compose run --detach \
    --name "$api_container" --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" \
    --env WEATHER_MIGRATION_AUTHORIZATION_RELEASE="$source_release" \
    --env WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256="$history_sha256" \
    api node apps/api/dist/main.js >/dev/null
  api_started=true

  # wait for every exact-source API read contract
  for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
    # accept only complete target-ledger reads
    if docker exec "$api_container" node -e \
      "const origin='http://127.0.0.1:3001';Promise.all([fetch(origin+'/api/v1/health'),fetch(origin+'/api/v1/sites')]).then(async([health,sites])=>{if(!health.ok||!sites.ok)process.exit(1);const healthBody=await health.json();const sitesBody=await sites.json();const site=sitesBody.data?.[0]?.slug;if(healthBody.data?.ready!==true||healthBody.data?.version!=='$source_release'||typeof site!=='string')process.exit(1);const path=origin+'/api/v1/sites/'+encodeURIComponent(site);const [current,history,forecast]=await Promise.all([fetch(path+'/current'),fetch(path+'/history?limit=1'),fetch(path+'/forecast')]);if(!current.ok||!history.ok||!forecast.ok)process.exit(1);const bodies=await Promise.all([current.json(),history.json(),forecast.json()]);if(bodies.some(body=>!Array.isArray(body.data))||bodies[2].data.length===0)process.exit(1)}).catch(()=>process.exit(1))"; then
      break
    fi
    sleep 1
  done
  ((_attempt < 30)) || die "fixed v13 source API compatibility failed"
  measure_fixed_v13_compatibility api "$candidate"

  # include teardown WAL before authorizing the active migration
  docker rm --force "$api_container" >/dev/null
  api_started=false
  docker rm --force "$provider_container" >/dev/null
  provider_started=false
  WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    dropdb --username postgres "$candidate" >/dev/null
  candidate_created=false
  measure_fixed_v13_compatibility teardown

  # publish only the target-bound proof payload after every source check
  write_migration_authorization \
    "$authorization_path" "$source_release" "$target_release" "$history_sha256"
)

# run previous-image checks on a disposable migrated clone
verify_previous_image_compatibility() (
  local target_env=$1
  local previous_env=$2
  local authorization_path=$3
  local compatibility_mode=${4:-legacy}
  local peak_output_path=${5:-}
  local candidate api_container target_api_container unproven_api_container provider_container provider_image
  local candidate_oid existing_candidate_oid api_container_id target_api_container_id
  local unproven_api_container_id provider_container_id
  local transient_container_id transient_container_name transient_container_started=false
  local transient_container_env transient_container_service
  local active_database provider_network previous_release target_release history_sha256
  local previous_history_sha256
  local invalid_history_sha256 unproven_status
  local non_compatibility_source_ids compatibility_source_count
  local anchor_migration_count export_migration_count runtime_provenance_migration_count
  local live_visibility_migration_count temperature_canary_migration_count rain_collection_migration_count station_access_migration_count rain_adjustment_migration_count adjustment_export_migration_count adjustment_maintenance_migration_count baseline_acl_verified
  local baseline_schema_state
  local migrations_changed=false
  local candidate_created=false
  local api_started=false
  local target_api_started=false
  local unproven_api_started=false
  local provider_started=false
  local before_successes after_successes
  local preexisting_data_contract preexisting_data_before preexisting_data_after
  local previous_api_snapshot target_api_snapshot catalog_consistent
  local capacity_samples capacity_monitor_completion capacity_monitor_pid postgres_dir
  local initial_free_bytes initial_free_inodes
  local capacity_last_sample_index capacity_last_sample_timestamp capacity_monitor_next_sequence
  local migration_phase_start_index=-1 migration_phase_start_timestamp=0
  local migration_phase_end_index=-1 migration_phase_end_timestamp=0
  local before_v4_records=0 after_v4_records=0
  local migration_started_milliseconds migration_duration_milliseconds=0
  candidate="weather_compat_$(date -u +%Y%m%d%H%M%S)_$$"
  api_container="${candidate}_api"
  target_api_container="${candidate}_target_api"
  unproven_api_container="${candidate}_api_unproven"
  provider_container="${candidate}_provider"
  provider_image=$(env_value "$target_env" WEATHER_SERVER_IMAGE)
  active_database=$(env_value "$previous_env" WEATHER_DATABASE_NAME)
  previous_release=$(env_value "$previous_env" WEATHER_RELEASE)
  target_release=$(env_value "$target_env" WEATHER_RELEASE)
  [[ "$compatibility_mode" == legacy || "$compatibility_mode" == normal-21-23 ]] ||
    die "unknown compatibility mode"
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    require_canonical_descendant "$peak_output_path" "$releases_dir" \
      "normal compatibility peak output"
    [[ -f "$peak_output_path" && ! -L "$peak_output_path" ]] ||
      die "normal compatibility peak output is invalid"
  fi
  invalid_history_sha256=$(printf '%064d' 0)
  provider_network="${WEATHER_COMPOSE_PROJECT_NAME:-weather}_provider_egress"
  candidate_oid=
  api_container_id=
  target_api_container_id=
  unproven_api_container_id=
  provider_container_id=
  transient_container_id=
  transient_container_name=
  transient_container_env=
  transient_container_service=
  capacity_monitor_pid=
  capacity_monitor_next_sequence=
  capacity_samples=
  capacity_monitor_completion=
  preexisting_data_contract=
  preexisting_data_before=
  preexisting_data_after=

  # append one exact foreground filesystem sample after the background writer stops
  append_compatibility_capacity_sample() {
    local free_bytes free_inodes captured_at
    read -r free_bytes _ free_inodes < <(
      stat --file-system --format='%a %S %d' "$postgres_dir" |
        awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }') || return 1
    captured_at=$(date -u +%s%3N) || return 1
    printf '%s %s %s %s\n' "$capacity_monitor_next_sequence" "$captured_at" \
      "$free_bytes" "$free_inodes" >>"$capacity_samples" || return 1
    capacity_last_sample_index=$capacity_monitor_next_sequence
    capacity_last_sample_timestamp=$captured_at
    capacity_monitor_next_sequence=$((capacity_monitor_next_sequence + 1))
  }

  # start one bounded sampler segment at the next exact sequence ordinal
  start_compatibility_capacity_monitor() {
    local start_sequence=$capacity_monitor_next_sequence
    [[ -z "$capacity_monitor_pid" && "$start_sequence" =~ ^[1-9][0-9]*$ ]] || return 1
    rm -f -- "$capacity_monitor_completion" || return 1
    (
      _next_sequence=$start_sequence
      # publish one exact completion handshake only on an intentional stop
      # shellcheck disable=SC2317,SC2329
      complete_capacity_monitor() {
        _completion_temporary="$capacity_monitor_completion.$BASHPID"
        printf '%s %s\n' "$BASHPID" "$_next_sequence" >"$_completion_temporary" || exit 1
        chmod 600 "$_completion_temporary" || exit 1
        mv -f -- "$_completion_temporary" "$capacity_monitor_completion" || exit 1
        exit 0
      }
      trap complete_capacity_monitor TERM
      # retain every observed free-byte and inode low-water sample
      for ((_sample = 0; _sample < 4093; _sample += 1)); do
        read -r _free_bytes _ _free_inodes < <(
          stat --file-system --format='%a %S %d' "$postgres_dir" |
            awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }') || exit 1
        # append each sample with one writer and one complete printf
        printf '%s %s %s %s\n' "$_next_sequence" "$(date -u +%s%3N)" \
          "$_free_bytes" "$_free_inodes" || exit 1
        _next_sequence=$((_next_sequence + 1))
        sleep 1
      done
      exit 1
    ) >>"$capacity_samples" &
    capacity_monitor_pid=$!
  }

  # stop one live sampler segment and retain its exact next ordinal
  stop_compatibility_capacity_monitor() {
    [[ -n "$capacity_monitor_pid" ]] || return 1
    stop_owned_capacity_monitor "$capacity_monitor_pid" \
      "$capacity_monitor_completion" "$capacity_samples" || return 1
    capacity_monitor_pid=
  }

  # remove one invocation-owned container only while its immutable identity matches
  remove_owned_compatibility_container() {
    local container_id=$1
    local container_name=$2
    local details
    [[ "$container_id" =~ ^[a-f0-9]{64}$ ]] || return 1
    details=$(docker container inspect --format '{{.Id}} {{.Name}}' "$container_id") || return 1
    [[ "$details" == "$container_id /$container_name" ]] || return 1
    docker container stop --time 10 "$container_id" >/dev/null || return 1
    details=$(docker container inspect --format '{{.Id}} {{.Name}}' "$container_id") || return 1
    [[ "$details" == "$container_id /$container_name" ]] || return 1
    docker container rm "$container_id" >/dev/null || return 1
    ! docker container inspect "$container_id" >/dev/null 2>&1
  }

  # prove one Compose task retains its exact generated identity
  require_owned_compatibility_task_container() {
    local container_id=$1
    local container_name=$2
    local environment=$3
    local service=$4
    local project=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
    local expected_reference expected_image_id details
    [[ "$container_id" =~ ^[a-f0-9]{64}$ &&
      ( "$service" == migration || "$service" == worker ) ]] || return 1
    expected_reference=$(env_value "$environment" WEATHER_SERVER_IMAGE) || return 1
    expected_image_id=$(docker image inspect --format '{{.Id}}' "$expected_reference") || return 1
    details=$(docker container inspect --format \
      '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
      "$container_id") || return 1
    [[ "$details" == "$container_id|/$container_name|$project|$service|$expected_reference|$expected_image_id" ]]
  }

  # remove one proven Compose task without touching a reused name or ID
  remove_owned_compatibility_task_container() {
    local container_id=$1
    local container_name=$2
    local environment=$3
    local service=$4
    require_owned_compatibility_task_container \
      "$container_id" "$container_name" "$environment" "$service" || return 1
    docker container stop --time 10 "$container_id" >/dev/null || return 1
    require_owned_compatibility_task_container \
      "$container_id" "$container_name" "$environment" "$service" || return 1
    docker container rm "$container_id" >/dev/null || return 1
    ! docker container inspect "$container_id" >/dev/null 2>&1
  }

  # run and remove one foreground-equivalent Compose task by exact container identity
  run_owned_compatibility_container() {
    local environment=$1
    local name=$2
    local service=$3
    local exit_status launched_id
    shift 3
    [[ "$service" == migration || "$service" == worker ]] ||
      die "compatibility task service is invalid"
    # reject any preexisting object at the invocation-owned name
    ! docker container inspect "$name" >/dev/null 2>&1 ||
      die "compatibility task name already exists"
    transient_container_name=$name
    transient_container_env=$environment
    transient_container_service=$service
    # recover an exact created ID even when Compose reports a launch failure
    if transient_container_id=$(WEATHER_ENV_FILE=$environment compose run --detach \
      --name "$name" "$@"); then
      :
    else
      launched_id=$(docker container inspect --format '{{.Id}}' "$name" 2>/dev/null) ||
        die "compatibility task launch failed before creation"
      transient_container_id=$launched_id
      transient_container_started=true
      require_owned_compatibility_task_container "$transient_container_id" \
        "$transient_container_name" "$transient_container_env" \
        "$transient_container_service" || die "failed compatibility task identity differs"
      remove_owned_compatibility_task_container "$transient_container_id" \
        "$transient_container_name" "$transient_container_env" \
        "$transient_container_service" || die "failed compatibility task cleanup failed"
      transient_container_started=false
      transient_container_id=
      die "compatibility task launch failed"
    fi
    # bind successful creation to the returned immutable ID before waiting
    if [[ ! "$transient_container_id" =~ ^[a-f0-9]{64}$ ]]; then
      launched_id=$(docker container inspect --format '{{.Id}}' "$name" 2>/dev/null) ||
        die "compatibility task returned an invalid identity"
      transient_container_id=$launched_id
    fi
    transient_container_started=true
    require_owned_compatibility_task_container "$transient_container_id" \
      "$transient_container_name" "$transient_container_env" \
      "$transient_container_service" || die "compatibility task identity differs"
    exit_status=$(docker container wait "$transient_container_id") ||
      die "compatibility task wait failed"
    # preserve bounded application diagnostics for a proven nonzero task
    if [[ "$exit_status" != 0 ]]; then
      docker container logs "$transient_container_id" >&2 || true
    fi
    remove_owned_compatibility_task_container "$transient_container_id" \
      "$transient_container_name" "$transient_container_env" \
      "$transient_container_service" || die "compatibility task cleanup failed"
    transient_container_started=false
    transient_container_id=
    transient_container_name=
    transient_container_env=
    transient_container_service=
    [[ "$exit_status" == 0 ]]
  }

  # clean every disposable compatibility resource
  # shellcheck disable=SC2317,SC2329
  cleanup_compatibility() {
    local status=$?
    local current_candidate_oid remaining_candidate_oid
    current_candidate_oid=
    remaining_candidate_oid=

    # remove a started API probe
    if [[ "$api_started" == true ]]; then
      remove_owned_compatibility_container "$api_container_id" "$api_container" || status=1
    fi

    # remove an interrupted foreground-equivalent task
    if [[ "$transient_container_started" == true ]]; then
      remove_owned_compatibility_task_container "$transient_container_id" \
        "$transient_container_name" "$transient_container_env" \
        "$transient_container_service" || status=1
    fi

    # remove a started target API probe
    if [[ "$target_api_started" == true ]]; then
      remove_owned_compatibility_container \
        "$target_api_container_id" "$target_api_container" || status=1
    fi

    # remove a started rejection probe
    if [[ "$unproven_api_started" == true ]]; then
      remove_owned_compatibility_container \
        "$unproven_api_container_id" "$unproven_api_container" || status=1
    fi

    # remove a started provider stub
    if [[ "$provider_started" == true ]]; then
      remove_owned_compatibility_container \
        "$provider_container_id" "$provider_container" || status=1
    fi

    # drop a created candidate last
    if [[ "$candidate_created" == true ]]; then
      current_candidate_oid=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
        psql --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
          --tuples-only --no-align --command \
          "SELECT oid FROM pg_database WHERE datname = '$candidate'") || status=1
      # recover the exact OID after a successful create whose first identity read failed
      if [[ ! "$candidate_oid" =~ ^[1-9][0-9]*$ &&
        "$current_candidate_oid" =~ ^[1-9][0-9]*$ ]]; then
        candidate_oid=$current_candidate_oid
      fi
      if [[ "$candidate_oid" =~ ^[1-9][0-9]*$ &&
        "$current_candidate_oid" == "$candidate_oid" ]]; then
        WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
          dropdb --username postgres "$candidate" >/dev/null || status=1
        remaining_candidate_oid=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
          psql --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
            --tuples-only --no-align --command \
            "SELECT oid FROM pg_database WHERE datname = '$candidate'") || status=1
        [[ -z "$remaining_candidate_oid" ]] || status=1
      else
        status=1
      fi
    fi

    # stop and seal the invocation-owned filesystem monitor after owned cleanup
    if [[ -n "$capacity_monitor_pid" ]]; then
      local monitor_failed=false
      # reject nonzero, early or incomplete sampler termination
      if ! stop_compatibility_capacity_monitor; then
        monitor_failed=true
      fi
      # append one final complete sample only after the background writer is gone
      if [[ "$monitor_failed" == false ]]; then
        append_compatibility_capacity_sample || monitor_failed=true
      fi
      if [[ "$monitor_failed" == true ]]; then
        status=1
      fi
      if [[ "$status" == 0 ]]; then
        if ! node --input-type=module - "$capacity_samples" "$peak_output_path" \
          "$migration_duration_milliseconds" "$migration_phase_start_index" \
          "$migration_phase_start_timestamp" "$migration_phase_end_index" \
          "$migration_phase_end_timestamp" <<'NODE'
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const [input, output, migrationDurationMilliseconds, migrationPhaseStartIndex,
  migrationPhaseStartTimestamp, migrationPhaseEndIndex,
  migrationPhaseEndTimestamp] = process.argv.slice(2);
const bytes = readFileSync(input);
if (bytes.length < 1 || bytes.length > 1_048_576) {
  throw new Error("normal compatibility capacity samples exceed their byte bound");
}
const lines = bytes.toString("utf8").trimEnd().split("\n");
if (lines.length < 2 || lines.length > 4_096) {
  throw new Error("normal compatibility capacity sample count is invalid");
}
const samples = lines.map((line) => {
  const fields = line.split(" ");
  if (fields.length !== 4 || fields.some((field) => !/^(?:0|[1-9]\d*)$/u.test(field))) {
    throw new Error("normal compatibility capacity sample is invalid");
  }
  const [sequence, capturedAtMilliseconds, freeBytes, freeInodes] = fields.map(Number);
  if (![sequence, capturedAtMilliseconds, freeBytes, freeInodes].every(Number.isSafeInteger)) {
    throw new Error("normal compatibility capacity sample exceeds exact integer bounds");
  }
  return { capturedAtMilliseconds, freeBytes, freeInodes, sequence };
});

// retain one ordered complete series
for (let index = 1; index < samples.length; index += 1) {
  if (samples[index].sequence !== index ||
    samples[index].capturedAtMilliseconds < samples[index - 1].capturedAtMilliseconds) {
    throw new Error("normal compatibility capacity sample clocks are unordered");
  }
}
if (samples[0].sequence !== 0) throw new Error("normal compatibility sample sequence is invalid");
const minimumFreeBytes = Math.min(...samples.map((sample) => sample.freeBytes));
const minimumFreeInodes = Math.min(...samples.map((sample) => sample.freeInodes));
const phaseStartIndex = Number(migrationPhaseStartIndex);
const phaseEndIndex = Number(migrationPhaseEndIndex);
if (!Number.isSafeInteger(phaseStartIndex) || !Number.isSafeInteger(phaseEndIndex) ||
  phaseStartIndex < 0 || phaseEndIndex <= phaseStartIndex || phaseEndIndex >= samples.length ||
  samples[phaseStartIndex].capturedAtMilliseconds !== Number(migrationPhaseStartTimestamp) ||
  samples[phaseEndIndex].capturedAtMilliseconds !== Number(migrationPhaseEndTimestamp)) {
  throw new Error("normal migration phase boundaries are invalid");
}
const migrationSamples = samples.slice(phaseStartIndex, phaseEndIndex + 1);
const migrationMinimumFreeBytes = Math.min(...migrationSamples.map((sample) => sample.freeBytes));
const migrationMinimumFreeInodes = Math.min(...migrationSamples.map((sample) => sample.freeInodes));
const body = {
  contractVersion: "normal-stage-capacity-samples/v1",
  initialFreeBytes: samples[0].freeBytes,
  initialFreeInodes: samples[0].freeInodes,
  minimumFreeBytes,
  minimumFreeInodes,
  migrationMinimumFreeBytes,
  migrationMinimumFreeInodes,
  migrationDurationMilliseconds: Number(migrationDurationMilliseconds),
  migrationPeakGrowthBytes: samples[phaseStartIndex].freeBytes - migrationMinimumFreeBytes,
  migrationPeakGrowthInodes: samples[phaseStartIndex].freeInodes - migrationMinimumFreeInodes,
  migrationPhaseEndIndex: phaseEndIndex,
  migrationPhaseEndTimestamp: Number(migrationPhaseEndTimestamp),
  migrationPhaseStartIndex: phaseStartIndex,
  migrationPhaseStartTimestamp: Number(migrationPhaseStartTimestamp),
  peakGrowthBytes: samples[0].freeBytes - minimumFreeBytes,
  peakGrowthInodes: samples[0].freeInodes - minimumFreeInodes,
  samples,
};
if (body.peakGrowthBytes < 0 || body.peakGrowthInodes < 0 ||
  body.migrationPeakGrowthBytes < 0 || body.migrationPeakGrowthInodes < 0) {
  throw new Error("normal compatibility capacity peak is invalid");
}
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
const receipt = { ...body,
  receiptSha256: createHash("sha256").update(canonical(body)).digest("hex") };
writeFileSync(output, canonical(receipt), { mode: 0o600 });
NODE
        then
          status=1
        fi
      fi
    fi
    # retain a stopped complete or partial normal series before unlinking evidence
    if [[ "$compatibility_mode" == normal-21-23 ]]; then
      if [[ "$status" != 0 ]]; then
        retain_release_failure_evidence normal-stage "$status" \
          "$capacity_samples" "$peak_output_path" "$preexisting_data_contract" \
          "$preexisting_data_before" "$preexisting_data_after" || status=1
      fi
      if [[ -n "$capacity_samples" ]]; then
        rm -f -- "$capacity_samples" "$capacity_monitor_completion"
      fi
    fi
    rm -f -- "$preexisting_data_contract" "$preexisting_data_before" \
      "$preexisting_data_after"

    trap - EXIT
    exit "$status"
  }
  trap cleanup_compatibility EXIT
  trap 'exit 130' HUP INT TERM

  # monitor the database filesystem through copy, migration and owned cleanup
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    postgres_dir=$(env_value "$previous_env" WEATHER_POSTGRES_DIR)
    capacity_samples=$(mktemp "$releases_dir/.normal-capacity-samples.XXXXXX")
    capacity_monitor_completion="$capacity_samples.complete"
    chmod 600 "$capacity_samples"
    read -r initial_free_bytes _ initial_free_inodes < <(
      stat --file-system --format='%a %S %d' "$postgres_dir" |
        awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }')
    printf '0 %s %s %s\n' "$(date -u +%s%3N)" \
      "$initial_free_bytes" "$initial_free_inodes" >"$capacity_samples"
    capacity_monitor_next_sequence=1
    preexisting_data_contract=$(mktemp "$releases_dir/.normal-data-contract.XXXXXX")
    preexisting_data_before=$(mktemp "$releases_dir/.normal-data-before.XXXXXX")
    preexisting_data_after=$(mktemp "$releases_dir/.normal-data-after.XXXXXX")
    chmod 600 "$preexisting_data_contract" "$preexisting_data_before" \
      "$preexisting_data_after"
    start_compatibility_capacity_monitor ||
      die "normal compatibility capacity monitor could not start"
  fi

  printf 'Creating disposable compatibility database %s...\n' "$candidate"
  existing_candidate_oid=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command \
      "SELECT oid FROM pg_database WHERE datname = '$candidate'") ||
    die "compatibility database absence check failed"
  [[ -z "$existing_candidate_oid" ]] ||
    die "compatibility database name already exists"
  # capture a partial create before rejecting its failed command status
  if ! WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    createdb --username postgres --owner weather_owner "$candidate"; then
    candidate_oid=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
        --tuples-only --no-align --command \
        "SELECT oid FROM pg_database WHERE datname = '$candidate'" 2>/dev/null || true)
    [[ "$candidate_oid" =~ ^[1-9][0-9]*$ ]] && candidate_created=true
    die "compatibility database creation failed"
  fi
  candidate_created=true
  candidate_oid=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command \
      "SELECT oid FROM pg_database WHERE datname = '$candidate'")
  [[ "$candidate_oid" =~ ^[1-9][0-9]*$ ]] ||
    die "compatibility database identity is invalid"

  WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    pg_dump --username weather_owner --dbname "$active_database" \
      --format=custom --no-owner |
    WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      pg_restore --username postgres --dbname "$candidate" \
        --no-owner --role weather_owner --exit-on-error

  # preserve exact schema 21 for the normal successor rehearsal
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    previous_history_sha256=$(migration_history_sha256 "$previous_env" "$candidate")
    [[ "$previous_history_sha256" == adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93 ]] ||
      die "normal compatibility source is not the exact schema-21 ledger"
    write_preexisting_data_contract "$previous_env" "$candidate" \
      "$preexisting_data_contract"
    write_preexisting_data_snapshot "$previous_env" "$candidate" \
      "$preexisting_data_contract" "$preexisting_data_before"
  else
    # restore the historical eight-migration boundary only for legacy staging
    WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
      --command "CREATE OR REPLACE FUNCTION weather_source_is_current(candidate_id bigint) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, public AS \$function\$ SELECT NOT EXISTS (SELECT 1 FROM public.sources candidate JOIN public.sources successor ON successor.station_id = candidate.station_id AND successor.active AND successor.material_provider_config->>'supersedesSourceKey' = candidate.source_key WHERE candidate.id = candidate_id); \$function\$; DROP INDEX IF EXISTS public.weather_records_forecast_product_valid_idx; DROP TRIGGER IF EXISTS weather_records_forecast_products_insert ON weather_records; DROP TRIGGER IF EXISTS weather_records_forecast_products_delete ON weather_records; DROP TRIGGER IF EXISTS weather_records_forecast_products_truncate ON weather_records; DROP FUNCTION IF EXISTS weather_sync_forecast_products_after_insert(); DROP FUNCTION IF EXISTS weather_sync_forecast_products_after_delete(); DROP FUNCTION IF EXISTS weather_sync_forecast_products_after_truncate(); DROP TABLE IF EXISTS forecast_products; DROP TABLE IF EXISTS adjustment_shadow_registration_windows_v3, adjustment_registration_horizons_v3, adjustment_registration_schedule_v3; DROP FUNCTION IF EXISTS adjustment_shadow_registration_slot_v3(text); DROP FUNCTION IF EXISTS weather_register_adjustment_shadow_v3(jsonb); DROP FUNCTION IF EXISTS weather_initialize_adjustment_registration_schedule_v3(jsonb); DROP FUNCTION IF EXISTS weather_guard_adjustment_registration_v3(); DROP FUNCTION IF EXISTS weather_reject_adjustment_registration_v3_mutation(); DROP TRIGGER IF EXISTS weather_records_clear_adjustment_revision_pointer_v1 ON weather_records; DROP TRIGGER IF EXISTS forecast_anchor_records_clear_adjustment_revision_pointer_v1 ON forecast_anchor_records; DROP TRIGGER IF EXISTS rain_adjustment_runs_immutable ON rain_adjustment_runs; DROP FUNCTION IF EXISTS weather_append_adjustment_shadow_v4(jsonb,text); DROP FUNCTION IF EXISTS adjustment_revision_frontier_v1(); DROP FUNCTION IF EXISTS adjustment_shadow_revision_admission_v1(text,text,text,integer); DROP FUNCTION IF EXISTS weather_clear_adjustment_revision_pointer_v1(); DROP FUNCTION IF EXISTS weather_issue_adjustment_revision_receipt_v1(text,text,text,text); DROP FUNCTION IF EXISTS weather_bind_weather_record_revisions_v1(jsonb); DROP FUNCTION IF EXISTS weather_bind_forecast_anchor_revisions_v1(jsonb); DROP FUNCTION IF EXISTS weather_guard_rain_adjustment_revision_v1(); DROP FUNCTION IF EXISTS weather_bind_rain_gate_revision_v1(jsonb); DROP FUNCTION IF EXISTS weather_bind_ecmwf_temperature_revision_v1(jsonb); DROP FUNCTION IF EXISTS adjustment_revision_serving_snapshot_v1(timestamptz,bigint); DROP FUNCTION IF EXISTS adjustment_weather_revision_admission_v1(jsonb); DROP FUNCTION IF EXISTS adjustment_forecast_anchor_revision_admission_v1(jsonb); DROP FUNCTION IF EXISTS adjustment_rain_gate_revision_admission_v1(jsonb); DROP FUNCTION IF EXISTS adjustment_ecmwf_temperature_revision_admission_v1(jsonb); DROP FUNCTION IF EXISTS weather_mark_adjustment_revision_gap_v1(text,jsonb,jsonb); ALTER TABLE weather_records DROP COLUMN IF EXISTS adjustment_revision_receipt; ALTER TABLE forecast_anchor_records DROP COLUMN IF EXISTS adjustment_revision_receipt; ALTER TABLE rain_adjustment_runs DROP COLUMN IF EXISTS adjustment_revision_receipt; ALTER TABLE ecmwf_temperature_canary_runs DROP COLUMN IF EXISTS adjustment_revision_receipt; DROP FUNCTION IF EXISTS weather_adjustment_revision_receipt_valid_v1(jsonb); DROP TABLE IF EXISTS adjustment_revision_frontier_v1; DROP SEQUENCE IF EXISTS adjustment_revision_ordinal_v1; DROP TABLE IF EXISTS adjustment_shadow_terminal_results_v2; DROP FUNCTION IF EXISTS weather_guard_adjustment_shadow_terminal_result_v2(); DROP FUNCTION IF EXISTS weather_record_adjustment_shadow_terminal_result_v2(jsonb); DROP FUNCTION IF EXISTS weather_retire_adjustment_shadow_registration_v2(jsonb); DROP FUNCTION IF EXISTS weather_append_adjustment_shadow_v3(jsonb,text); DROP TABLE IF EXISTS adjustment_shadow_predictions_v2, adjustment_confirmation_accesses_v2, adjustment_shadow_registrations_v2; DROP FUNCTION IF EXISTS weather_guard_adjustment_shadow_registration_v2(); DROP FUNCTION IF EXISTS weather_guard_adjustment_shadow_prediction_v2(); DROP FUNCTION IF EXISTS weather_guard_adjustment_confirmation_access_v2(); DROP FUNCTION IF EXISTS weather_reject_adjustment_maintenance_mutation(); DROP FUNCTION IF EXISTS weather_register_adjustment_shadow_v2(jsonb); DROP FUNCTION IF EXISTS weather_append_adjustment_temperature_shadow_v2(jsonb); DROP FUNCTION IF EXISTS weather_append_adjustment_wind_shadow_v2(jsonb); DROP FUNCTION IF EXISTS weather_append_adjustment_rain_shadow_v2(jsonb); DROP FUNCTION IF EXISTS adjustment_shadow_body_admission_v2(text,text,text,integer); DROP FUNCTION IF EXISTS weather_finalize_adjustment_shadow_metadata_v2(jsonb); DROP FUNCTION IF EXISTS weather_record_adjustment_confirmation_access_v2(jsonb); DROP FUNCTION IF EXISTS adjustment_confirmation_availability_v2(text); DROP FUNCTION IF EXISTS adjustment_confirmation_export_v2(text,text,smallint); DROP VIEW IF EXISTS adjustment_evaluation_export_manifest_v1; DROP VIEW IF EXISTS adjustment_evaluation_export_rows_v1; DROP VIEW IF EXISTS rain_collection_status_v1; DROP TABLE IF EXISTS rain_adjustment_runs; DROP FUNCTION IF EXISTS weather_guard_rain_adjustment_run(); DROP TABLE IF EXISTS rain_capture_receipts; DROP TABLE IF EXISTS rain_capture_claims; DROP FUNCTION IF EXISTS weather_guard_rain_capture_claim(); DROP FUNCTION IF EXISTS weather_guard_rain_capture_receipt(); DROP FUNCTION IF EXISTS weather_reject_rain_capture_mutation(); DROP VIEW IF EXISTS forecast_runtime_provenance_v1; DROP VIEW IF EXISTS forecast_training_export_manifest_v1; DROP VIEW IF EXISTS forecast_training_export_rows_v1; DROP TABLE IF EXISTS ecmwf_temperature_canary_hours; DROP TABLE IF EXISTS ecmwf_temperature_canary_runs; DROP FUNCTION IF EXISTS weather_guard_ecmwf_temperature_canary_run_update(); DROP FUNCTION IF EXISTS weather_require_ecmwf_temperature_canary_hour_identity(); DROP FUNCTION IF EXISTS weather_reject_ecmwf_temperature_canary_hour_mutation(); DROP TABLE IF EXISTS forecast_anchor_records; DROP FUNCTION IF EXISTS weather_require_historical_forecast_anchor_source(); DROP FUNCTION IF EXISTS weather_guard_forecast_anchor_record_update(); GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA public TO PUBLIC; ALTER DEFAULT PRIVILEGES FOR ROLE weather_owner IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO PUBLIC; REVOKE SELECT (capabilities) ON sources FROM weather_api; DELETE FROM schema_migrations WHERE name IN ('0009_forecast_anchor_records.sql', '0010_forecast_training_export.sql', '0011_forecast_runtime_provenance.sql', '0012_hide_archive_only_forecasts_from_live_reads.sql', '0013_ecmwf_temperature_canary.sql', '0014_rain_collection.sql', '0015_rain_station_access.sql', '0016_rain_adjustment.sql', '0017_adjustment_evaluation_export.sql', '0018_adjustment_maintenance_v2.sql', '0019_adjustment_maintenance_recurring.sql', '0020_adjustment_revision_frontier.sql', '0021_adjustment_rolling_registration.sql', '0022_forecast_product_catalog.sql', '0023_forecast_product_read_index.sql')"
  baseline_schema_state=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*)::text || ':' || COALESCE(to_regclass('forecast_anchor_records')::text, '') || ':' || COALESCE(to_regclass('forecast_training_export_rows_v1')::text, '') || ':' || COALESCE(to_regclass('ecmwf_temperature_canary_runs')::text, '') || ':' || COALESCE(to_regclass('ecmwf_temperature_canary_hours')::text, '') || ':' || COALESCE(to_regclass('rain_capture_claims')::text, '') || ':' || COALESCE(to_regclass('rain_capture_receipts')::text, '') || ':' || COALESCE(to_regclass('rain_collection_status_v1')::text, '') FROM schema_migrations")
  [[ "$baseline_schema_state" == "8:::::::" ]] ||
    die "previous compatibility database does not match the eight-migration Git baseline"
    previous_history_sha256=$(migration_history_sha256 "$previous_env" "$candidate")
  fi

  # start the activation-equivalent phase only after the full clone is allocated
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    stop_compatibility_capacity_monitor ||
      die "normal compatibility capacity monitor ended before migration"
    append_compatibility_capacity_sample ||
      die "normal compatibility migration baseline sample failed"
    migration_phase_start_index=$capacity_last_sample_index
    migration_phase_start_timestamp=$capacity_last_sample_timestamp
    start_compatibility_capacity_monitor ||
      die "normal compatibility migration phase monitor could not start"
  fi
  migration_started_milliseconds=$(date -u +%s%3N)
  # keep both additive forecast migrations in one transaction
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    run_owned_compatibility_container "$target_env" "${candidate}_migration" migration --no-deps \
      --env WEATHER_DATABASE_NAME="$candidate" migration \
      node deploy/scripts/migrate.mjs --atomic-normal-21-23
  else
    run_owned_compatibility_container "$target_env" "${candidate}_migration" migration --no-deps \
      --env WEATHER_DATABASE_NAME="$candidate" migration
  fi
  migration_duration_milliseconds=$(($(date -u +%s%3N) - migration_started_milliseconds))
  if [[ "$compatibility_mode" == normal-21-23 ]] &&
    ((migration_duration_milliseconds > 60000)); then
    die "normal schema-23 compatibility migration exceeded 60000ms"
  fi
  history_sha256=$(migration_history_sha256 "$previous_env" "$candidate")

  # require the exact additive schema-23 successor and unchanged preexisting data
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    [[ "$history_sha256" == 1db25d8a515e85b4c71c3a3dbe4a1f61ebdfdf14e095a24b307daa039dfb5201 ]] ||
      die "normal compatibility target is not the exact schema-23 ledger"
    write_preexisting_data_snapshot "$previous_env" "$candidate" \
      "$preexisting_data_contract" "$preexisting_data_after"
    verify_preexisting_data_snapshots "$preexisting_data_contract" \
      "$preexisting_data_before" "$preexisting_data_after" ||
      die "normal compatibility migration changed preexisting application data"
  fi

  # identify a real trailing migration compatibility run
  if [[ "$history_sha256" != "$previous_history_sha256" ]]; then
    migrations_changed=true
    anchor_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0009_forecast_anchor_records.sql'")
    [[ "$anchor_migration_count" == 1 ]] ||
      die "forecast anchor migration is missing from the compatibility database"
    export_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0010_forecast_training_export.sql'")
    [[ "$export_migration_count" == 1 ]] ||
      die "forecast training export migration is missing from the compatibility database"
    runtime_provenance_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0011_forecast_runtime_provenance.sql'")
    [[ "$runtime_provenance_migration_count" == 1 ]] ||
      die "forecast runtime provenance migration is missing from the compatibility database"
    live_visibility_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0012_hide_archive_only_forecasts_from_live_reads.sql'")
    [[ "$live_visibility_migration_count" == 1 ]] ||
      die "forecast live visibility migration is missing from the compatibility database"
    # require the isolated sidecar migration after rebuilding the baseline
    temperature_canary_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0013_ecmwf_temperature_canary.sql'")
    [[ "$temperature_canary_migration_count" == 1 ]] ||
      die "ECMWF temperature canary migration is missing from the compatibility database"
    # require rain collection after rebuilding the baseline
    rain_collection_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0014_rain_collection.sql'")
    [[ "$rain_collection_migration_count" == 1 ]] ||
      die "rain collection migration is missing from the compatibility database"
    # require the station access guard after rebuilding the baseline
    station_access_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0015_rain_station_access.sql'")
    [[ "$station_access_migration_count" == 1 ]] ||
      die "rain station access migration is missing from the compatibility database"
    # require the retained bounded rain projection after rebuilding the baseline
    rain_adjustment_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0016_rain_adjustment.sql'")
    [[ "$rain_adjustment_migration_count" == 1 ]] ||
      die "rain adjustment migration is missing from the compatibility database"
    # require the private adjustment evaluation export after rebuilding the baseline
    adjustment_export_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0017_adjustment_evaluation_export.sql'")
    [[ "$adjustment_export_migration_count" == 1 ]] ||
      die "adjustment evaluation export migration is missing from the compatibility database"
    # require the closed maintenance schema after rebuilding the baseline
    adjustment_maintenance_migration_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM schema_migrations WHERE name = '0018_adjustment_maintenance_v2.sql'")
    [[ "$adjustment_maintenance_migration_count" == 1 ]] ||
      die "adjustment maintenance migration is missing from the compatibility database"
  fi

  non_compatibility_source_ids=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT COALESCE(string_agg(s.id::text, ',' ORDER BY s.id), '') FROM sources s JOIN providers p ON p.id = s.provider_id WHERE s.active AND p.provider_key <> 'open-meteo'")
  [[ -z "$non_compatibility_source_ids" ||
    "$non_compatibility_source_ids" =~ ^[0-9]+(,[0-9]+)*$ ]] ||
    die "non-compatibility source identity is invalid"
  compatibility_source_count=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM sources s JOIN providers p ON p.id = s.provider_id WHERE s.active AND p.provider_key = 'open-meteo'")
  [[ "$compatibility_source_count" =~ ^[1-9][0-9]*$ ]] ||
    die "Open-Meteo compatibility source is missing"
  # apply the ACL version matching the candidate schema
  if [[ "$migrations_changed" == true ]]; then
    apply_runtime_database_acl "$previous_env" "$candidate"
    verify_runtime_database_acl "$previous_env" "$candidate"
  else
    WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
        <"$deploy_dir/postgres/runtime-acl-v1.sql"
    baseline_acl_verified=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
        --tuples-only --no-align --command "SELECT has_table_privilege('weather_api', 'sites', 'SELECT') AND NOT has_column_privilege('weather_api', 'sources', 'capabilities', 'SELECT') AND has_table_privilege('weather_ingest', 'weather_records', 'INSERT') AND to_regclass('forecast_anchor_records') IS NULL")
    [[ "$baseline_acl_verified" == t ]] ||
      die "previous runtime ACL verification failed"
  fi
  # end the activation-equivalent phase before compatibility workers add fixture data
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    stop_compatibility_capacity_monitor ||
      die "normal compatibility capacity monitor ended during migration"
    append_compatibility_capacity_sample ||
      die "normal compatibility migration final sample failed"
    migration_phase_end_index=$capacity_last_sample_index
    migration_phase_end_timestamp=$capacity_last_sample_timestamp
    start_compatibility_capacity_monitor ||
      die "normal compatibility post-migration monitor could not start"
  fi
  WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
      --command "UPDATE ingestion_checkpoints SET last_committed_at = TIMESTAMPTZ '1970-01-01 00:00:00+00'; UPDATE sources SET cadence_seconds = 60 WHERE active"

  # isolate the worker to the deterministic provider stub
  if [[ -n "$non_compatibility_source_ids" ]]; then
    WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
        --command "UPDATE sources SET active = false WHERE id IN ($non_compatibility_source_ids)"
  fi

  provider_container_id=$(docker run --detach --name "$provider_container" \
    --network "$provider_network" --network-alias "$provider_container" \
    --env WEATHER_COMPATIBILITY_DYNAMIC_FORECAST=1 \
    "$provider_image" node deploy/scripts/compatibility-provider.mjs)
  provider_started=true
  [[ "$provider_container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "compatibility provider identity is invalid"

  # wait for the deterministic provider
  for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
    # stop after the first successful readiness response
    if docker exec "$provider_container" node -e \
      "fetch('http://127.0.0.1:3002/health').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"; then
      break
    fi
    sleep 1
  done
  ((_attempt < 30)) || die "compatibility provider did not become ready"

  before_successes=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM ingestion_runs WHERE state='succeeded'")

  # snapshot the legacy v4 product only for a migrated-schema proof
  if [[ "$migrations_changed" == true ]]; then
    before_v4_records=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM weather_records wr JOIN sources s ON s.id = wr.source_id WHERE s.source_key = 'open-meteo-forecast-v4'")
  fi

  run_owned_compatibility_container "$previous_env" "${candidate}_worker" worker --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" \
    --env WEATHER_MIGRATION_AUTHORIZATION_RELEASE="$previous_release" \
    --env WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256="$history_sha256" \
    --env WEATHER_OPEN_METEO_COMPATIBILITY_ORIGIN="http://$provider_container:3002" \
    worker node apps/worker/dist/worker.js --once
  after_successes=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
    psql --username postgres --dbname "$candidate" --tuples-only --no-align \
      --command "SELECT count(*) FROM ingestion_runs WHERE state='succeeded'")

  # require the previous worker to insert legacy v4 rows after migration
  if [[ "$migrations_changed" == true ]]; then
    after_v4_records=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT count(*) FROM weather_records wr JOIN sources s ON s.id = wr.source_id WHERE s.source_key = 'open-meteo-forecast-v4'")
    [[ "$before_v4_records" =~ ^[0-9]+$ && "$after_v4_records" =~ ^[0-9]+$ ]] ||
      die "previous worker returned an invalid legacy v4 row count"

    # require at least one newly persisted forecast row
    if (( after_v4_records <= before_v4_records )); then
      die "previous worker did not insert a legacy v4 forecast row after migration"
    fi
  fi

  # prove the old worker maintained the complete exact forecast catalog
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    catalog_consistent=$(WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT NOT EXISTS ((SELECT DISTINCT wr.source_id, wr.product_run_at FROM weather_records wr WHERE wr.source_kind = 'forecast' EXCEPT SELECT fp.source_id, fp.product_run_at FROM forecast_products fp) UNION ALL (SELECT fp.source_id, fp.product_run_at FROM forecast_products fp EXCEPT SELECT DISTINCT wr.source_id, wr.product_run_at FROM weather_records wr WHERE wr.source_kind = 'forecast'))")
    [[ "$catalog_consistent" == t ]] ||
      die "previous worker left the schema-23 forecast catalog inconsistent"
  fi

  # expose only the bounded persisted failure diagnosis
  if (( after_successes <= before_successes )); then
    WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --username postgres --dbname "$candidate" --tuples-only --no-align \
        --command "SELECT COALESCE(error_code, 'unknown') || ': ' || COALESCE(error_message, 'no detail') FROM ingestion_runs WHERE state = 'failed' ORDER BY completed_at DESC NULLS LAST, id DESC LIMIT 1" >&2
    die "previous worker compatibility failed"
  fi

  # restore other providers for previous API reads
  if [[ -n "$non_compatibility_source_ids" ]]; then
    WEATHER_ENV_FILE=$previous_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
        --command "UPDATE sources SET active = true WHERE id IN ($non_compatibility_source_ids)"
  fi

  # prove authorization only for trailing history
  if [[ "$migrations_changed" == true ]]; then
    # reuse the refreshed worker heartbeat
    if run_owned_compatibility_container \
      "$previous_env" "${candidate}_worker_unproven" worker --no-deps \
      --env WEATHER_DATABASE_NAME="$candidate" \
      worker node apps/worker/dist/health.js >/dev/null 2>&1; then
      die "previous worker accepted unproven migration history"
    fi

    unproven_api_container_id=$(WEATHER_ENV_FILE=$previous_env compose run --detach \
      --name "$unproven_api_container" --no-deps \
      --env WEATHER_DATABASE_NAME="$candidate" \
      --env WEATHER_MIGRATION_AUTHORIZATION_RELEASE="$previous_release" \
      --env WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256="$invalid_history_sha256" \
      api node apps/api/dist/main.js)
    unproven_api_started=true
    [[ "$unproven_api_container_id" =~ ^[a-f0-9]{64}$ ]] ||
      die "unproven compatibility API identity is invalid"

    # require the previous API to reject a wrong digest
    for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
      # inspect the first reachable health response
      if unproven_status=$(docker exec "$unproven_api_container" node -e \
        "fetch('http://127.0.0.1:3001/api/v1/health').then(r=>console.log(r.status)).catch(()=>process.exit(1))" 2>/dev/null); then
        [[ "$unproven_status" == 503 ]] ||
          die "previous API accepted invalid migration authorization"
        break
      fi
      sleep 1
    done
    ((_attempt < 30)) || die "previous API did not reject invalid migration authorization"
    remove_owned_compatibility_container \
      "$unproven_api_container_id" "$unproven_api_container" ||
      die "unproven compatibility API cleanup failed"
    unproven_api_started=false
  fi

  api_container_id=$(WEATHER_ENV_FILE=$previous_env compose run --detach \
    --name "$api_container" --no-deps \
    --env WEATHER_DATABASE_NAME="$candidate" \
    --env WEATHER_MIGRATION_AUTHORIZATION_RELEASE="$previous_release" \
    --env WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256="$history_sha256" \
    api node apps/api/dist/main.js)
  api_started=true
  [[ "$api_container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "compatibility API identity is invalid"

  # wait for every previous API read contract
  for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
    # accept only complete read success
    if docker exec --env WEATHER_REQUIRE_FORECAST_READ="$migrations_changed" \
      "$api_container" node -e \
      "const origin='http://127.0.0.1:3001';Promise.all([fetch(origin+'/api/v1/health'),fetch(origin+'/api/v1/sites')]).then(async([health,sites])=>{if(!health.ok||!sites.ok)process.exit(1);const healthBody=await health.json();const sitesBody=await sites.json();const site=sitesBody.data?.[0]?.slug;if(healthBody.data?.ready!==true||healthBody.data?.version!=='$previous_release'||typeof site!=='string')process.exit(1);const sitePath=origin+'/api/v1/sites/'+encodeURIComponent(site);const [current,history,forecast]=await Promise.all([fetch(sitePath+'/current'),fetch(sitePath+'/history?limit=1'),fetch(sitePath+'/forecast')]);if(!current.ok||!history.ok||!forecast.ok)process.exit(1);const currentBody=await current.json();const historyBody=await history.json();const forecastBody=await forecast.json();if(!Array.isArray(currentBody.data)||!Array.isArray(historyBody.data)||!Array.isArray(forecastBody.data))process.exit(1);if(process.env.WEATHER_REQUIRE_FORECAST_READ==='true'&&forecastBody.data.length===0)process.exit(1)}).catch(()=>process.exit(1))"; then
      break
    fi
    sleep 1
  done

  ((_attempt < 30)) || die "previous API compatibility failed"

  # compare the new API's user-visible reads against the old API in one frozen clone
  if [[ "$compatibility_mode" == normal-21-23 ]]; then
    target_api_container_id=$(WEATHER_ENV_FILE=$target_env compose run --detach \
      --name "$target_api_container" \
      --no-deps --env WEATHER_DATABASE_NAME="$candidate" \
      api node apps/api/dist/main.js)
    target_api_started=true
    [[ "$target_api_container_id" =~ ^[a-f0-9]{64}$ ]] ||
      die "target compatibility API identity is invalid"
    for ((_attempt = 0; _attempt < 30; _attempt += 1)); do
      if docker exec "$target_api_container" node -e \
        "fetch('http://127.0.0.1:3001/api/v1/health').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"; then
        break
      fi
      sleep 1
    done
    ((_attempt < 30)) || die "target API compatibility failed"
    previous_api_snapshot=$(docker exec "$api_container" node -e \
      "const o='http://127.0.0.1:3001';fetch(o+'/api/v1/sites').then(async r=>{if(!r.ok)throw Error();const sites=await r.json();const s=sites.data?.[0]?.slug;if(typeof s!=='string')throw Error();const p=o+'/api/v1/sites/'+encodeURIComponent(s);const rs=await Promise.all(['/current','/history?limit=1','/forecast'].map(x=>fetch(p+x)));if(rs.some(x=>!x.ok))throw Error();console.log(JSON.stringify([sites,...await Promise.all(rs.map(x=>x.json()))]))}).catch(()=>process.exit(1))")
    target_api_snapshot=$(docker exec "$target_api_container" node -e \
      "const o='http://127.0.0.1:3001';fetch(o+'/api/v1/sites').then(async r=>{if(!r.ok)throw Error();const sites=await r.json();const s=sites.data?.[0]?.slug;if(typeof s!=='string')throw Error();const p=o+'/api/v1/sites/'+encodeURIComponent(s);const rs=await Promise.all(['/current','/history?limit=1','/forecast'].map(x=>fetch(p+x)));if(rs.some(x=>!x.ok))throw Error();console.log(JSON.stringify([sites,...await Promise.all(rs.map(x=>x.json()))]))}).catch(()=>process.exit(1))")
    [[ "$previous_api_snapshot" == "$target_api_snapshot" ]] ||
      die "schema-21 and schema-23 API reads differ"
  fi

  # publish only after every real compatibility check
  write_migration_authorization \
    "$authorization_path" "$previous_release" "$target_release" "$history_sha256"
)

# reconcile retained PostgreSQL administrator authority
start_postgres() {
  local env_file=$1
  WEATHER_ENV_FILE=$env_file compose up -d --no-deps --force-recreate --wait postgres ||
    return 1
}

# capture one running supported writer by immutable Compose and image identity
capture_supported_writer_container() {
  local env_file=$1
  local service=$2
  local project=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
  local container_id expected_reference expected_image_id details
  [[ "$service" == api || "$service" == worker ]] ||
    die "unsupported writer service identity"
  container_id=$(WEATHER_ENV_FILE=$env_file compose ps -q "$service")
  [[ "$container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "supported writer container identity is invalid: $service"
  expected_reference=$(env_value "$env_file" WEATHER_SERVER_IMAGE)
  expected_image_id=$(docker image inspect --format '{{.Id}}' "$expected_reference")
  details=$(docker container inspect --format \
    '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}|{{.State.Running}}' \
    "$container_id")
  [[ "$details" == "$container_id|/$project-$service-1|$project|$service|$expected_reference|$expected_image_id|true" ]] ||
    die "supported writer container differs from active release: $service"
  printf '%s\n' "$container_id"
}

# stop one previously captured supported writer without replacing its identity
stop_supported_writer_container() {
  local env_file=$1
  local service=$2
  local container_id=$3
  local project=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
  local expected_reference expected_image_id details
  expected_reference=$(env_value "$env_file" WEATHER_SERVER_IMAGE)
  expected_image_id=$(docker image inspect --format '{{.Id}}' "$expected_reference")
  details=$(docker container inspect --format \
    '{{.Id}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
    "$container_id") || return 1
  [[ "$details" == "$container_id|$project|$service|$expected_reference|$expected_image_id" ]] ||
    return 1
  docker container stop --time 30 "$container_id" >/dev/null || return 1
  details=$(docker container inspect --format \
    '{{.Id}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.State.Running}}' \
    "$container_id") || return 1
  [[ "$details" == "$container_id|$project|$service|false" ]]
}

# require no supported application writer or one-off migration remains active
require_supported_writers_quiesced() {
  local env_file=$1
  local project=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
  local database_name service running activity_count
  database_name=$(env_value "$env_file" WEATHER_DATABASE_NAME)
  # reject daemon and one-off containers for every supported write service
  for service in api worker migration; do
    running=$(docker ps --quiet \
      --filter "label=com.docker.compose.project=$project" \
      --filter "label=com.docker.compose.service=$service")
    [[ -z "$running" ]] || die "supported writer remains active: $service"
  done
  activity_count=$(WEATHER_ENV_FILE=$env_file compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command \
      "SELECT count(*) FROM pg_stat_activity WHERE usename IN ('weather_api', 'weather_ingest')")
  [[ "$activity_count" == 0 ]] ||
    die "supported database writer sessions remain active"
}

# publish one exact normal-activation migration ownership journal
write_normal_activation_journal() {
  local source_release=$1
  local target_release=$2
  local container_name=$3
  local container_id=$4
  local state=$5
  local temporary
  validate_release "$source_release"
  validate_release "$target_release"
  [[ "$source_release" != "$target_release" &&
    "$container_name" =~ ^weather_normal_migration_[0-9_]+-[1-9][0-9]?_[1-9][0-9]*$ &&
    ( "$container_id" == absent || "$container_id" =~ ^[a-f0-9]{64}$ ) &&
    ( "$state" == prepared || "$state" == running || "$state" == removed ) &&
    ( "$state" != prepared || "$container_id" == absent ) &&
    ( "$state" == prepared || "$container_id" != absent ) ]] ||
    die "normal activation migration journal identity is invalid"
  temporary=$(mktemp "$state_dir/.normal-activation-v1.XXXXXX")
  chmod 600 "$temporary"
  printf '%s\n' \
    "WEATHER_NORMAL_ACTIVATION_SOURCE_RELEASE=$source_release" \
    "WEATHER_NORMAL_ACTIVATION_TARGET_RELEASE=$target_release" \
    "WEATHER_NORMAL_ACTIVATION_CONTAINER_NAME=$container_name" \
    "WEATHER_NORMAL_ACTIVATION_CONTAINER_ID=$container_id" \
    "WEATHER_NORMAL_ACTIVATION_STATE=$state" >"$temporary"
  sync -f "$temporary"
  mv -f -- "$temporary" "$normal_activation_journal"
  chmod 600 "$normal_activation_journal"
  sync -f "$state_dir"
}

# validate and print one exact normal-activation migration journal
read_normal_activation_journal() {
  local source_release target_release container_name container_id state
  require_file "$normal_activation_journal"
  [[ ! -L "$normal_activation_journal" &&
    "$(stat -c '%u:%a' "$normal_activation_journal")" == "$EUID:600" &&
    "$(grep -cE '^[A-Z][A-Z0-9_]*=' "$normal_activation_journal")" == 5 ]] ||
    die "normal activation migration journal metadata is invalid"
  grep -qEv '^(WEATHER_NORMAL_ACTIVATION_SOURCE_RELEASE|WEATHER_NORMAL_ACTIVATION_TARGET_RELEASE|WEATHER_NORMAL_ACTIVATION_CONTAINER_NAME|WEATHER_NORMAL_ACTIVATION_CONTAINER_ID|WEATHER_NORMAL_ACTIVATION_STATE)=' \
    "$normal_activation_journal" &&
    die "normal activation migration journal contains an unknown field"
  source_release=$(env_value "$normal_activation_journal" \
    WEATHER_NORMAL_ACTIVATION_SOURCE_RELEASE)
  target_release=$(env_value "$normal_activation_journal" \
    WEATHER_NORMAL_ACTIVATION_TARGET_RELEASE)
  container_name=$(env_value "$normal_activation_journal" \
    WEATHER_NORMAL_ACTIVATION_CONTAINER_NAME)
  container_id=$(env_value "$normal_activation_journal" \
    WEATHER_NORMAL_ACTIVATION_CONTAINER_ID)
  state=$(env_value "$normal_activation_journal" WEATHER_NORMAL_ACTIVATION_STATE)
  validate_release "$source_release"
  validate_release "$target_release"
  [[ "$source_release" != "$target_release" &&
    "$container_name" =~ ^weather_normal_migration_[0-9_]+-[1-9][0-9]?_[1-9][0-9]*$ &&
    ( "$container_id" == absent || "$container_id" =~ ^[a-f0-9]{64}$ ) &&
    ( "$state" == prepared || "$state" == running || "$state" == removed ) &&
    ( "$state" != prepared || "$container_id" == absent ) &&
    ( "$state" == prepared || "$container_id" != absent ) ]] ||
    die "normal activation migration journal contents are invalid"
  printf '%s %s %s %s %s\n' "$source_release" "$target_release" \
    "$container_name" "$container_id" "$state"
}

# stop and remove only the migration container bound by the activation journal
quiesce_normal_activation_migration() {
  local activation_env=$1
  local source_release target_release container_name container_id state
  local inspected_id details project expected_reference expected_image_id running
  read -r source_release target_release container_name container_id state < <(
    read_normal_activation_journal)
  project=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
  expected_reference=$(env_value "$activation_env" WEATHER_SERVER_IMAGE)
  expected_image_id=$(docker image inspect --format '{{.Id}}' "$expected_reference")
  # a prepared journal may precede Docker container creation
  if [[ "$state" == prepared ]]; then
    inspected_id=$(docker container inspect --format '{{.Id}}' "$container_name" 2>/dev/null || true)
    if [[ -z "$inspected_id" ]]; then
      return 0
    fi
    container_id=$inspected_id
  fi
  # a removed journal must name an already absent container
  if [[ "$state" == removed ]]; then
    ! docker container inspect "$container_id" >/dev/null 2>&1 || return 1
    ! docker container inspect "$container_name" >/dev/null 2>&1 || return 1
    return 0
  fi
  details=$(docker container inspect --format \
    '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
    "$container_id") || return 1
  [[ "$details" == "$container_id|/$container_name|$project|migration|$expected_reference|$expected_image_id" ]] ||
    return 1
  running=$(docker container inspect --format '{{.State.Running}}' "$container_id") || return 1
  # stop a live migration before reading the final database ledger
  if [[ "$running" == true ]]; then
    docker container stop --time 30 "$container_id" >/dev/null || return 1
  fi
  [[ "$(docker container inspect --format '{{.State.Running}}' "$container_id")" == false ]] ||
    return 1
  docker container rm "$container_id" >/dev/null || return 1
  ! docker container inspect "$container_id" >/dev/null 2>&1 || return 1
  ! docker container inspect "$container_name" >/dev/null 2>&1 || return 1
  write_normal_activation_journal "$source_release" "$target_release" \
    "$container_name" "$container_id" removed
}

# stop one owned sampler only through its exact successful completion handshake
stop_owned_capacity_monitor() {
  local monitor_pid=$1
  local completion_path=$2
  local samples_path=$3
  local completion_pid completion_sequence sample_line_count
  capacity_monitor_next_sequence=
  kill -TERM "$monitor_pid" >/dev/null 2>&1 || return 1
  wait "$monitor_pid" >/dev/null 2>&1 || return 1
  [[ -f "$completion_path" && ! -L "$completion_path" ]] || return 1
  read -r completion_pid completion_sequence <"$completion_path" || return 1
  sample_line_count=$(wc -l <"$samples_path") || return 1
  [[ "$completion_pid" == "$monitor_pid" &&
    "$completion_sequence" =~ ^[1-9][0-9]*$ &&
    "$sample_line_count" == "$completion_sequence" ]] || return 1
  # reject missing, repeated or reordered sequence ordinals before receipt parsing
  awk '($1 != NR - 1) { exit 1 } END { if (NR == 0) exit 1 }' "$samples_path" ||
    return 1
  capacity_monitor_next_sequence=$completion_sequence
}

# restore normal runtime authority from one already-quiesced exact live ledger
restore_normal_activation_after_quiescence() {
  local current_env=$1
  local runtime_release=$2
  local source_release=$3
  local target_release=$4
  local actual_history authorization_path
  actual_history=$(migration_history_sha256 \
    "$current_env" "$(env_value "$current_env" WEATHER_DATABASE_NAME)") || return 1
  # repair a pre-SQL or transactionally rolled-back failure to source authority
  if [[ "$actual_history" == adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93 &&
    "$runtime_release" == "$source_release" ]]; then
    write_private_state "$state_dir/schema-release" "$source_release" || return 1
    restore_images "$current_env" "$source_release" "$source_release" || return 1
    return 0
  fi
  # retain target authority after a committed migration or later bootstrap/ACL failure
  if [[ "$actual_history" == 1db25d8a515e85b4c71c3a3dbe4a1f61ebdfdf14e095a24b307daa039dfb5201 ]]; then
    authorization_path=$(migration_authorization "$target_release") || return 1
    validate_migration_authorization "$authorization_path" \
      "$source_release" "$target_release" || return 1
    write_private_state "$state_dir/schema-release" "$target_release" || return 1
    restore_images "$current_env" "$runtime_release" "$target_release" || return 1
    return 0
  fi
  return 1
}

# recover one committed normal schema using its exact staged authority
recover_committed_normal_release() {
  local runtime_release=$1
  local schema_release=$2
  local runtime_env target_env source_release source_env authorization_path stage_receipt
  local actual_history
  authorization_path=$(migration_authorization "$schema_release")
  validate_migration_authorization "$authorization_path" \
    "$(env_value "$authorization_path" WEATHER_MIGRATION_AUTHORIZATION_RELEASE)" \
    "$schema_release"
  source_release=$(env_value "$authorization_path" WEATHER_MIGRATION_AUTHORIZATION_RELEASE)
  [[ ( "$runtime_release" == "$source_release" || "$runtime_release" == "$schema_release" ) &&
    "$source_release" != "$schema_release" ]] ||
    die "normal recovery runtime differs from staged authority"
  source_env=$(release_env "$source_release")
  target_env=$(release_env "$schema_release")
  runtime_env=$(release_env "$runtime_release")
  validate_release_env "$source_env" "$source_release"
  validate_release_env "$target_env" "$schema_release"
  validate_release_env "$runtime_env" "$runtime_release"
  require_control_plane_compatibility "$source_env"
  require_control_plane_compatibility "$target_env"
  require_control_plane_compatibility "$runtime_env"
  stage_receipt="$releases_dir/$schema_release.normal-stage-receipt.json"
  require_file "$stage_receipt"
  start_postgres "$runtime_env" || die "normal recovery PostgreSQL restart failed"
  actual_history=$(migration_history_sha256 \
    "$runtime_env" "$(env_value "$runtime_env" WEATHER_DATABASE_NAME)")
  [[ "$actual_history" == 1db25d8a515e85b4c71c3a3dbe4a1f61ebdfdf14e095a24b307daa039dfb5201 ]] ||
    die "normal recovery requires the exact complete schema-23 ledger"
  read_normal_stage_qualification "$source_env" "$target_env" "$stage_receipt" >/dev/null
  restore_images "$runtime_env" "$runtime_release" "$schema_release"
  write_active_symlink "$runtime_release"
}

# provision the persistent Xweather usage ledger
prepare_xweather_usage_directory() {
  install -d -m 0750 -o 10002 -g 10002 /var/lib/weather/xweather
}

# run one restore body with absent or exact persisted migration authority
with_restore_migration_authorization() (
  local authorization_path=$1
  local runtime_release=$2
  local schema_release=$3
  shift 3
  unset WEATHER_MIGRATION_AUTHORIZATION_RELEASE
  unset WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256
  local WEATHER_MIGRATION_AUTHORIZATION_RELEASE
  local WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256

  # inject only a nonempty validated authorization identity
  if [[ -n "$authorization_path" ]]; then
    validate_migration_authorization \
      "$authorization_path" "$runtime_release" "$schema_release"
    WEATHER_MIGRATION_AUTHORIZATION_RELEASE=$runtime_release
    WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=$(env_value \
      "$authorization_path" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)
    export WEATHER_MIGRATION_AUTHORIZATION_RELEASE
    export WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256
  fi
  "$@"
)

# restore the ordinary runtime after authorization isolation
restore_images_runtime() {
  local env_file=$1
  prepare_xweather_usage_directory || return 1
  start_postgres "$env_file" || return 1
  apply_runtime_database_acl \
    "$env_file" "$(env_value "$env_file" WEATHER_DATABASE_NAME)" || return 1
  verify_runtime_database_acl \
    "$env_file" "$(env_value "$env_file" WEATHER_DATABASE_NAME)" || return 1
  WEATHER_ENV_FILE=$env_file compose up -d --no-deps --wait \
    api worker web cloudflared || return 1
}

# restore one exact image set without migration
restore_images() (
  local env_file=$1
  local runtime_release=${2:-}
  local schema_release=${3:-}
  local restore_authorization_path

  # reject partial lifecycle identity
  if [[ -n "$runtime_release" || -n "$schema_release" ]]; then
    [[ -n "$runtime_release" && -n "$schema_release" ]] ||
      die "runtime and schema releases must be provided together"
  fi

  # inject only a validated older-release authorization
  if [[ -n "$runtime_release" && "$runtime_release" != "$schema_release" ]]; then
    restore_authorization_path=$(migration_authorization "$schema_release")
  fi
  with_restore_migration_authorization "${restore_authorization_path:-}" \
    "$runtime_release" "$schema_release" restore_images_runtime "$env_file"
)

# require the exact pre-epoch core used by dashboard recovery
require_dashboard_core_runtime_identity() {
  local env_file=$1
  [[ "$(env_value "$env_file" WEATHER_RELEASE)" == "$recurring_previous_release" &&
    "$(env_value "$env_file" WEATHER_CONTROL_PLANE_VERSION)" == 13 &&
    "$(env_value "$env_file" WEATHER_CONTROL_PLANE_SHA256)" == \
      "$recurring_previous_control_plane_sha256" &&
    "$(env_value "$env_file" WEATHER_SERVER_IMAGE)" == \
      "ghcr.io/anstosa/weather-server@sha256:fb140b46d6eaea463ba2d10dc74303eac515135a37c21ddb746cdce744fd23ab" &&
    "$(env_value "$env_file" WEATHER_WEB_IMAGE)" == \
      "ghcr.io/anstosa/weather-web@sha256:fdcb2d10da4c9ed5ec8651bafa96c9d2b240b66b85db85619b909d6e144e2d7b" ]] ||
    die "dashboard recovery core runtime identity differs"
}

# freeze every immutable core and non-web runtime identity
write_dashboard_core_snapshot() {
  local env_file=$1
  local output=$2
  local include_runtime=${3:-true}
  local current previous schema active environment_sha256 history settings
  local service key expected_reference container details configured_reference image_id expected_id platform
  [[ "$include_runtime" == true || "$include_runtime" == false ]] ||
    die "dashboard core snapshot mode is invalid"
  require_dashboard_core_runtime_identity "$env_file"
  current=$(read_release_state "$state_dir/current-release")
  previous=$(read_optional_release_state "$state_dir/previous-release")
  schema=$(read_release_state "$state_dir/schema-release")
  active=$(readlink "$state_dir/active.env")
  [[ "$current" == "$recurring_previous_release" &&
    "$schema" == "$recurring_previous_release" &&
    "$active" == "../releases/$recurring_previous_release.env" ]] ||
    die "dashboard core snapshot marker tuple differs"
  environment_sha256=$(sha256sum "$env_file" | awk '{print $1}')
  require_v14_compatibility_bridge_migration_ledger \
    "$env_file" "$(env_value "$env_file" WEATHER_DATABASE_NAME)"
  history=$(migration_history_sha256 \
    "$env_file" "$(env_value "$env_file" WEATHER_DATABASE_NAME)")
  [[ "$history" =~ ^[a-f0-9]{64}$ ]] || die "dashboard migration history hash is invalid"
  settings=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    inert-v14-settings-snapshot)
  [[ "$settings" =~ ^[0-9]+:[0-9]+\ [a-f0-9]{64}$ ]] ||
    die "dashboard settings snapshot is invalid"
  umask 077
  printf '%s\n' \
    "ACTIVE_ENV=$active" \
    "CORE_ENV_SHA256=$environment_sha256" \
    "CURRENT_RELEASE=$current" \
    "PREVIOUS_RELEASE=${previous:-absent}" \
    "SCHEMA_RELEASE=$schema" \
    "MIGRATION_HISTORY_SHA256=$history" \
    "SETTINGS_SNAPSHOT=$settings" >"$output"

  # bind every core setting that a web-only release must preserve
  for key in WEATHER_SERVER_IMAGE WEATHER_WEB_IMAGE POSTGRES_IMAGE CLOUDFLARED_IMAGE \
    WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH \
    WEATHER_CONTROL_PLANE_SHA256 WEATHER_CONTROL_PLANE_VERSION; do
    printf '%s=%s\n' "$key" "$(env_value "$env_file" "$key")" >>"$output"
  done

  # bind every locally resolved core image independently of container lifetime
  for key in WEATHER_SERVER_IMAGE WEATHER_WEB_IMAGE POSTGRES_IMAGE CLOUDFLARED_IMAGE; do
    expected_reference=$(env_value "$env_file" "$key")
    expected_id=$(docker image inspect --format '{{.Id}}' "$expected_reference")
    platform=$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$expected_reference")
    [[ "$expected_id" =~ ^sha256:[a-f0-9]{64}$ && "$platform" == linux/arm64 ]] ||
      die "dashboard core local image identity is invalid: $key"
    printf 'LOCAL_%s=%s|%s\n' "$key" "$expected_id" "$platform" >>"$output"
  done

  # stable recovery proofs stop before process-local container identities
  if [[ "$include_runtime" == false ]]; then
    chmod 600 "$output"
    return
  fi

  # bind all persistent non-web containers to their configured and local images
  for service in postgres migration api worker cloudflared; do
    container=$(WEATHER_ENV_FILE=$env_file compose ps --all --quiet "$service")
    [[ -z "$container" || "$container" =~ ^[a-f0-9]{12,64}$ ]] ||
      die "dashboard core container identity is invalid: $service"
    if [[ -z "$container" ]]; then
      [[ "$service" == migration ]] ||
        die "dashboard core container is missing: $service"
      printf 'CONTAINER_%s=absent\n' "$service" >>"$output"
      continue
    fi
    case "$service" in
      postgres) key=POSTGRES_IMAGE ;;
      migration|api|worker) key=WEATHER_SERVER_IMAGE ;;
      cloudflared) key=CLOUDFLARED_IMAGE ;;
    esac
    expected_reference=$(env_value "$env_file" "$key")
    details=$(docker container inspect --format '{{.Config.Image}}|{{.Image}}' "$container")
    IFS='|' read -r configured_reference image_id <<<"$details"
    expected_id=$(docker image inspect --format '{{.Id}}' "$expected_reference")
    [[ "$configured_reference" == "$expected_reference" &&
      "$image_id" == "$expected_id" && "$image_id" =~ ^sha256:[a-f0-9]{64}$ ]] ||
      die "dashboard core container image differs: $service"
    printf 'CONTAINER_%s=%s|%s|%s\n' \
      "$service" "$container" "$configured_reference" "$image_id" >>"$output"
  done
  chmod 600 "$output"
}

# hash one complete core snapshot after validation
dashboard_core_snapshot_sha256() {
  local env_file=$1
  local output=$2
  local include_runtime=${3:-true}
  write_dashboard_core_snapshot "$env_file" "$output" "$include_runtime"
  sha256sum "$output" | awk '{print $1}'
}

# restore the exact schema-18 core without future mounts, ACLs or web
restore_dashboard_core_runtime() (
  local env_file=$1
  local override database
  require_dashboard_core_runtime_identity "$env_file"
  database=$(env_value "$env_file" WEATHER_DATABASE_NAME)
  WEATHER_ENV_FILE=$env_file compose up -d --no-deps --no-recreate --wait postgres ||
    return 1
  require_v14_compatibility_bridge_migration_ledger "$env_file" "$database" || return 1
  override=$(mktemp "$state_dir/.dashboard-core-recovery.XXXXXX.yaml") ||
    die "dashboard core recovery override could not be created"
  trap 'rm -f -- "$override"' EXIT
  write_v14_compatibility_compose_override "$override"
  require_v14_compatibility_compose_override "$env_file" "$override"
  v14_compatibility_compose "$env_file" "$override" \
    up -d --no-deps --no-recreate --wait api worker cloudflared
)

# restore the pre-epoch bridge runtime after authorization isolation
restore_v14_compatibility_bridge_runtime() {
  local env_file=$1
  local schema_release=$2
  prepare_xweather_usage_directory || return 1
  start_postgres "$env_file" || return 1
  # apply only the already-authorized full-schema ACL
  if [[ "$schema_release" == "$full_v14_release" ]]; then
    apply_runtime_database_acl \
      "$env_file" "$(env_value "$env_file" WEATHER_DATABASE_NAME)" || return 1
    verify_runtime_database_acl \
      "$env_file" "$(env_value "$env_file" WEATHER_DATABASE_NAME)" || return 1
  fi
  start_v14_compatibility_bridge_runtime "$env_file"
}

# restore the bridge image without requiring future epoch or catalog files
restore_v14_compatibility_bridge_images() (
  local env_file=$1
  local runtime_release=$2
  local schema_release=$3
  local restore_authorization_path
  [[ "$runtime_release" == "$v14_compatibility_bridge_release" &&
    ( "$schema_release" == "$v14_compatibility_bridge_release" ||
      "$schema_release" == "$full_v14_release" ) ]] ||
    die "v14 compatibility bridge restoration tuple differs"

  # inject only the persisted full-schema authorization
  if [[ "$schema_release" == "$full_v14_release" ]]; then
    restore_authorization_path=$(migration_authorization "$full_v14_release")
  fi
  with_restore_migration_authorization "${restore_authorization_path:-}" \
    "$runtime_release" "$schema_release" restore_v14_compatibility_bridge_runtime \
    "$env_file" "$schema_release"
)

# start an exact release without compatibility state
start_exact_release() (
  local env_file=$1
  unset WEATHER_MIGRATION_AUTHORIZATION_RELEASE
  unset WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256
  prepare_xweather_usage_directory || return 1
  WEATHER_ENV_FILE=$env_file compose up -d --remove-orphans --wait || return 1
)

# validate one package-produced dashboard capacity receipt
require_dashboard_capacity_receipt() {
  local receipt=$1
  local source_image=$2
  local target_image=$3
  node --input-type=module - "$receipt" "$source_image" "$target_image" <<'NODE'
import { readFileSync } from "node:fs";

const [path, sourceImage, targetImage] = process.argv.slice(2);
const value = JSON.parse(readFileSync(path, "utf8"));
const sourceDigest = sourceImage.slice(sourceImage.indexOf("@") + 1);
const targetDigest = targetImage.slice(targetImage.indexOf("@") + 1);
const digests = new Map((value.imageDigests ?? []).map(
  // bind each physical digest to its closed ownership role
  (entry) => [`${entry.role}/${entry.runtime}`, entry.digest],
));

// enforce the closed capacity decision needed by the shell caller
if (value?.contractVersion !== "dashboard-web-release-capacity/v1" ||
  value.compensationScope !== "fixed-core-web-only-source-restore" ||
  value.sourceRelease !== "2026.10.09-1" ||
  value.imageDigests?.length !== 6 || digests.size !== 6 ||
  digests.get("source/web") !== sourceDigest ||
  digests.get("target/web") !== targetDigest ||
  digests.get("compensating/web") !== sourceDigest ||
  value.state !== "capacity_ready" || value.retirementCreditBytes !== 0 ||
  !Number.isSafeInteger(value.requiredFreeBytes) || value.requiredFreeBytes < 0 ||
  !Number.isSafeInteger(value.requiredFreeInodes) || value.requiredFreeInodes < 0) {
  throw new Error("dashboard web release capacity receipt is invalid");
}
NODE
}

# validate one package-produced dashboard publication proof
require_dashboard_publication_proof() {
  local receipt=$1
  local target_release=$2
  local expected_core=$3
  node --input-type=module - "$receipt" "$target_release" "$expected_core" <<'NODE'
import { readFileSync } from "node:fs";

const [path, targetRelease, expectedCore] = process.argv.slice(2);
const hash = /^[a-f0-9]{40}$/u;
const value = JSON.parse(readFileSync(path, "utf8"));

// accept only a successful immutable public web publication
if (value?.contractVersion !== "dashboard-web-release-proof/v1" ||
  value.coreRelease !== expectedCore || value.targetRelease !== targetRelease ||
  value.state !== "publication_ready" || !hash.test(value.sourceCommit) ||
  !hash.test(value.targetCommit) ||
  !Number.isSafeInteger(value.checkRunId) || value.checkRunId < 1 ||
  !Number.isSafeInteger(value.publishRunId) || value.publishRunId < 1) {
  throw new Error("dashboard web release publication proof is invalid");
}
NODE
}

# capture one validated package decision without trusting console output
dashboard_package_receipt() {
  local output=$1
  shift
  umask 077
  : >"$output"
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" "$@" >"$output"
  [[ "$(stat -c '%a' "$output")" == 600 && "$(wc -c <"$output")" -le 65536 ]] ||
    die "dashboard package receipt is not private and bounded"
}

# verify the running web container and its public read boundary
verify_dashboard_web_runtime() {
  local env_file=$1
  local release=$2
  local expected_image=$3
  local container configured_image actual_image expected_id architecture details
  container=$(WEATHER_ENV_FILE=$env_file compose ps --quiet web)
  [[ "$container" =~ ^[a-f0-9]{12,64}$ ]] ||
    die "dashboard web container identity is invalid"
  details=$(docker container inspect --format '{{.Config.Image}}|{{.Image}}' "$container")
  IFS='|' read -r configured_image actual_image <<<"$details"
  expected_id=$(docker image inspect --format '{{.Id}}' "$expected_image")
  [[ "$configured_image" == "$expected_image" &&
    "$actual_image" == "$expected_id" && "$actual_image" =~ ^sha256:[a-f0-9]{64}$ ]] ||
    die "dashboard web container image digest differs"
  architecture=$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$expected_image")
  [[ "$architecture" == linux/arm64 ]] || die "dashboard web image is not linux/arm64"
  WEATHER_ENV_FILE=$env_file compose exec -T web node -e \
    "const origin='http://127.0.0.1:3000';Promise.all([fetch(origin+'/'),fetch(origin+'/api/v1/health'),fetch(origin+'/api/v1/sites')]).then(async([html,health,sites])=>{if(!html.ok||!health.ok||!sites.ok)process.exit(1);const text=await html.text();const healthBody=await health.json();const sitesBody=await sites.json();const site=sitesBody.data?.[0]?.slug;if(!text.includes('/assets/$release/')||healthBody.data?.ready!==true||typeof site!=='string')process.exit(1);const root=origin+'/api/v1/sites/'+encodeURIComponent(site);const [current,forecast]=await Promise.all([fetch(root+'/current'),fetch(root+'/forecast')]);if(!current.ok||!forecast.ok)process.exit(1);const bodies=await Promise.all([current.json(),forecast.json()]);if(bodies.some(body=>!Array.isArray(body.data)))process.exit(1)}).catch(()=>process.exit(1))" ||
    die "dashboard web health or readable API verification failed"
}

# start only the web service under one isolated environment
start_dashboard_web() {
  local env_file=$1
  WEATHER_ENV_FILE=$env_file compose up -d --no-deps --wait web
}

# publish one closed dashboard recovery journal
write_dashboard_release_journal() {
  local action=$1
  local core=$2
  local source_release=$3
  local target_release=$4
  local original_current=$5
  local original_previous=$6
  local source_env=$7
  local target_env=$8
  local source_image=$9
  local target_image=${10}
  local proof_sha256=${11}
  local core_snapshot_sha256=${12}
  local temporary
  [[ "$action" == release || "$action" == rollback ]] ||
    die "dashboard journal action is invalid"
  for release in "$core" "$source_release" "$target_release"; do
    validate_release "$release"
  done
  [[ "$original_current" == none || "$original_current" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]?$ ]] ||
    die "dashboard original current release is invalid"
  [[ "$original_previous" == none || "$original_previous" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]?$ ]] ||
    die "dashboard original previous release is invalid"
  validate_image_reference "$source_image"
  validate_image_reference "$target_image"
  [[ "$proof_sha256" =~ ^[a-f0-9]{64}$ ]] || die "dashboard proof hash is invalid"
  [[ "$core_snapshot_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "dashboard core snapshot hash is invalid"
  temporary=$(mktemp "$state_dir/.dashboard-release-v1.XXXXXX.pending")
  umask 077
  printf '%s\n' \
    "WEATHER_DASHBOARD_JOURNAL_VERSION=$dashboard_release_journal_version" \
    "WEATHER_DASHBOARD_JOURNAL_ACTION=$action" \
    "WEATHER_DASHBOARD_CORE_RELEASE=$core" \
    "WEATHER_DASHBOARD_SOURCE_RELEASE=$source_release" \
    "WEATHER_DASHBOARD_TARGET_RELEASE=$target_release" \
    "WEATHER_DASHBOARD_ORIGINAL_CURRENT=$original_current" \
    "WEATHER_DASHBOARD_ORIGINAL_PREVIOUS=$original_previous" \
    "WEATHER_DASHBOARD_SOURCE_ENV=$source_env" \
    "WEATHER_DASHBOARD_TARGET_ENV=$target_env" \
    "WEATHER_DASHBOARD_SOURCE_IMAGE=$source_image" \
    "WEATHER_DASHBOARD_TARGET_IMAGE=$target_image" \
    "WEATHER_DASHBOARD_PROOF_SHA256=$proof_sha256" \
    "WEATHER_DASHBOARD_CORE_SNAPSHOT_SHA256=$core_snapshot_sha256" >"$temporary"
  chmod 600 "$temporary"
  sync -f "$temporary"
  mv "$temporary" "$dashboard_release_journal"
  sync -f "$state_dir"
  validate_dashboard_release_journal
}

# validate the exact pending dashboard transaction format
validate_dashboard_release_journal() {
  local meaningful_lines allowed_fields key value core source target
  local source_env target_env expected_source_env expected_target_env
  require_canonical_descendant "$dashboard_release_journal" "$state_dir" "dashboard release journal"
  require_file "$dashboard_release_journal"
  [[ "$(stat -c '%u %a' "$dashboard_release_journal")" == "$EUID 600" ]] ||
    die "dashboard release journal must be owner-private"
  meaningful_lines=$(grep -cE '^[A-Z][A-Z0-9_]*=' "$dashboard_release_journal")
  allowed_fields='^WEATHER_DASHBOARD_(JOURNAL_VERSION|JOURNAL_ACTION|CORE_RELEASE|SOURCE_RELEASE|TARGET_RELEASE|ORIGINAL_CURRENT|ORIGINAL_PREVIOUS|SOURCE_ENV|TARGET_ENV|SOURCE_IMAGE|TARGET_IMAGE|PROOF_SHA256|CORE_SNAPSHOT_SHA256)='
  [[ "$meaningful_lines" -eq 13 ]] || die "dashboard release journal format is invalid"
  grep -qEv "$allowed_fields" "$dashboard_release_journal" &&
    die "dashboard release journal contains unknown data"
  [[ "$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_JOURNAL_VERSION)" == "$dashboard_release_journal_version" ]] ||
    die "dashboard release journal version is unsupported"
  value=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_JOURNAL_ACTION)
  [[ "$value" == release || "$value" == rollback ]] || die "dashboard journal action is invalid"
  for key in WEATHER_DASHBOARD_CORE_RELEASE WEATHER_DASHBOARD_SOURCE_RELEASE WEATHER_DASHBOARD_TARGET_RELEASE; do
    validate_release "$(env_value "$dashboard_release_journal" "$key")"
  done
  core=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_RELEASE)
  source=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_RELEASE)
  target=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_RELEASE)
  for key in WEATHER_DASHBOARD_ORIGINAL_CURRENT WEATHER_DASHBOARD_ORIGINAL_PREVIOUS; do
    value=$(env_value "$dashboard_release_journal" "$key")
    [[ "$value" == none ]] || validate_release "$value"
  done
  for key in WEATHER_DASHBOARD_SOURCE_IMAGE WEATHER_DASHBOARD_TARGET_IMAGE; do
    validate_image_reference "$(env_value "$dashboard_release_journal" "$key")"
  done
  [[ "$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_PROOF_SHA256)" =~ ^[a-f0-9]{64}$ ]] ||
    die "dashboard journal proof hash is invalid"
  [[ "$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_SNAPSHOT_SHA256)" =~ ^[a-f0-9]{64}$ ]] ||
    die "dashboard journal core snapshot hash is invalid"
  source_env=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_ENV)
  target_env=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_ENV)
  expected_source_env=$(release_env "$source")
  expected_target_env=$(release_env "$target")

  # select web environments only for releases distinct from the core
  if [[ "$source" != "$core" ]]; then
    expected_source_env=$(dashboard_release_env "$source")
    validate_dashboard_release_env "$source_env" "$source" "$core"
  else
    validate_release_env "$source_env" "$core"
  fi
  if [[ "$target" != "$core" ]]; then
    expected_target_env=$(dashboard_release_env "$target")
    validate_dashboard_release_env "$target_env" "$target" "$core"
  else
    validate_release_env "$target_env" "$core"
  fi
  [[ "$source_env" == "$expected_source_env" && "$target_env" == "$expected_target_env" ]] ||
    die "dashboard journal environment path differs"
  [[ "$(env_value "$source_env" WEATHER_WEB_IMAGE)" == \
      "$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_IMAGE)" &&
    "$(env_value "$target_env" WEATHER_WEB_IMAGE)" == \
      "$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_IMAGE)" ]] ||
    die "dashboard journal web image differs"
}

# reauthorize one pending transaction against its immutable publication
require_dashboard_journal_publication_proof() (
  local core target expected_sha256 temporary actual_sha256
  validate_dashboard_release_journal
  core=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_RELEASE)
  target=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_RELEASE)
  expected_sha256=$(env_value \
    "$dashboard_release_journal" WEATHER_DASHBOARD_PROOF_SHA256)
  temporary=$(mktemp "${TMPDIR:-/tmp}/weather-dashboard-recovery-proof.XXXXXX.json")

  # remove the transient public proof on every exit
  trap 'rm -f -- "$temporary"' EXIT
  dashboard_package_receipt "$temporary" web-release-proof "$target" "$core"
  require_dashboard_publication_proof "$temporary" "$target" "$core"
  actual_sha256=$(sha256sum "$temporary" | awk '{print $1}')
  [[ "$actual_sha256" == "$expected_sha256" ]] ||
    die "dashboard recovery publication proof changed"
  rm -f -- "$temporary"
  trap - EXIT
)

# restore the marker snapshot recorded before one dashboard transaction
restore_dashboard_marker_snapshot() {
  local original_current=$1
  local original_previous=$2

  # restore or remove the prior current override
  if [[ "$original_current" == none ]]; then
    remove_dashboard_state "$state_dir/active-web.env"
    remove_dashboard_state "$state_dir/current-web-release"
  else
    write_dashboard_active_symlink "$original_current"
    write_private_state "$state_dir/current-web-release" "$original_current"
  fi

  # restore or remove the prior rollback marker
  if [[ "$original_previous" == none ]]; then
    remove_dashboard_state "$state_dir/previous-web-release"
  else
    write_private_state "$state_dir/previous-web-release" "$original_previous"
  fi
}

# commit one web-only marker transaction with current written last
record_dashboard_release_success() {
  local target=$1
  local source=$2
  local core=$3

  # remove the override after returning to the unchanged core web
  if [[ "$target" == "$core" ]]; then
    remove_dashboard_state "$state_dir/active-web.env"
    remove_dashboard_state "$state_dir/previous-web-release"
    remove_dashboard_state "$state_dir/current-web-release"
    return
  fi
  write_private_state "$state_dir/previous-web-release" "$source"
  write_dashboard_active_symlink "$target"
  write_private_state "$state_dir/current-web-release" "$target"
}

# reconcile only a transaction whose current marker reached a known boundary
reconcile_dashboard_release_journal() {
  local core source target original_current original_previous current previous expected_link
  validate_dashboard_release_journal
  core=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_RELEASE)
  source=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_SOURCE_RELEASE)
  target=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_TARGET_RELEASE)
  original_current=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_ORIGINAL_CURRENT)
  original_previous=$(env_value "$dashboard_release_journal" WEATHER_DASHBOARD_ORIGINAL_PREVIOUS)
  require_dashboard_core_cas "$core"
  current=$(read_optional_release_state "$state_dir/current-web-release")
  previous=$(read_optional_release_state "$state_dir/previous-web-release")

  # retain a fully committed non-core target
  if [[ "$target" != "$core" && "$current" == "$target" ]]; then
    expected_link="../releases/$target.web.env"
    [[ "$previous" == "$source" && -L "$state_dir/active-web.env" &&
      "$(readlink "$state_dir/active-web.env")" == "$expected_link" ]] ||
      die "committed dashboard marker transaction is incomplete"
  # retain a fully committed return to core
  elif [[ "$target" == "$core" && -z "$current" ]]; then
    [[ -z "$previous" && ! -e "$state_dir/active-web.env" &&
      ! -L "$state_dir/active-web.env" ]] ||
      die "committed dashboard rollback markers are incomplete"
  else
    [[ "$current" == "${original_current/none/}" ]] ||
      die "dashboard recovery current marker is outside the transaction"
    restore_dashboard_marker_snapshot "$original_current" "$original_previous"
  fi
}

# reapply only the committed web override after core recovery succeeds
restore_dashboard_override_after_core_health() {
  local core=$1
  local current_web web_env image
  current_web=$(read_optional_release_state "$state_dir/current-web-release")

  # leave the restored core web active without override state
  if [[ -z "$current_web" ]]; then
    [[ ! -e "$state_dir/active-web.env" && ! -L "$state_dir/active-web.env" ]] ||
      die "dashboard active link exists without a committed override"
    web_env=$(release_env "$core")
    image=$(env_value "$web_env" WEATHER_WEB_IMAGE)
    start_dashboard_web "$web_env" || die "dashboard core web recovery start failed"
    verify_dashboard_web_runtime "$web_env" "$core" "$image"
    return
  fi
  web_env=$(dashboard_source_environment "$core" "$current_web")
  image=$(env_value "$web_env" WEATHER_WEB_IMAGE)
  start_dashboard_web "$web_env" || die "dashboard override recovery start failed"
  verify_dashboard_web_runtime "$web_env" "$current_web" "$image"
}

# deploy one immutable web image without touching the core runtime
dashboard_release() (
  local target_release=$1
  local expected_core=$2
  local expected_source=$3
  local core_env source_env target_env source_image target_image web_source
  local current_web previous_web
  local original_current original_previous proof_hash core_snapshot_before core_snapshot_after
  local stable_snapshot_before stable_snapshot_after
  local status=0 started=false
  local temporary proof_before proof_after capacity_before capacity_after
  temporary=$(mktemp -d "${TMPDIR:-/tmp}/weather-dashboard-release.XXXXXX")

  # compensate every post-journal failure with the exact source web
  # shellcheck disable=SC2317,SC2329
  cleanup_dashboard_release() {
    status=$?
    if [[ "$started" == true ]]; then
      if ! start_dashboard_web "$source_env" ||
        ! verify_dashboard_web_runtime "$source_env" "$expected_source" "$source_image" ||
        ! restore_dashboard_marker_snapshot "$original_current" "$original_previous"; then
        status=1
      else
        remove_dashboard_state "$dashboard_release_journal" || status=1
      fi
    fi
    rm -rf -- "$temporary"
    trap - EXIT
    exit "$status"
  }
  trap cleanup_dashboard_release EXIT
  require_dashboard_core_cas "$expected_core"
  [[ ! -e "$dashboard_release_journal" && ! -L "$dashboard_release_journal" ]] ||
    die "dashboard release recovery is required"
  core_env=$(release_env "$expected_core")
  target_env=$(dashboard_release_env "$target_release")
  current_web=$(read_optional_release_state "$state_dir/current-web-release")
  previous_web=$(read_optional_release_state "$state_dir/previous-web-release")

  # return success only for an exact already-committed retry
  if [[ "$current_web" == "$target_release" ]]; then
    [[ "$previous_web" == "$expected_source" ]] ||
      die "dashboard retry source does not match committed rollback state"
    validate_dashboard_release_env "$target_env" "$target_release" "$expected_core"
    target_image=$(env_value "$target_env" WEATHER_WEB_IMAGE)
    dashboard_package_receipt "$temporary/proof.after" web-release-proof \
      "$target_release" "$expected_core"
    require_dashboard_publication_proof \
      "$temporary/proof.after" "$target_release" "$expected_core"
    dashboard_core_snapshot_sha256 "$core_env" "$temporary/core.retry" >/dev/null
    verify_dashboard_web_runtime "$target_env" "$target_release" "$target_image"
    printf 'Dashboard web release %s is already active.\n' "$target_release"
    started=false
    trap - EXIT
    rm -rf -- "$temporary"
    return
  fi
  source_env=$(dashboard_source_environment "$expected_core" "$expected_source")
  source_image=$(env_value "$source_env" WEATHER_WEB_IMAGE)
  verify_dashboard_web_runtime "$source_env" "$expected_source" "$source_image"
  core_snapshot_before=$(dashboard_core_snapshot_sha256 \
    "$core_env" "$temporary/core.before")
  stable_snapshot_before=$(dashboard_core_snapshot_sha256 \
    "$core_env" "$temporary/core-stable.before" false)
  [[ "$target_release" != "$expected_core" ]] ||
    die "dashboard release target must differ from the core release"
  web_source="$(image_repository "$source_image"):$target_release"
  target_image=$(resolve_arm64_image "$web_source")

  # reuse only exact immutable target state
  if [[ -e "$target_env" || -L "$target_env" ]]; then
    validate_dashboard_release_env "$target_env" "$target_release" "$expected_core"
    [[ "$(env_value "$target_env" WEATHER_WEB_IMAGE)" == "$target_image" ]] ||
      die "dashboard release environment image differs"
  else
    write_dashboard_release_env "$core_env" "$target_env" "$target_release" "$target_image"
  fi
  cleanup_obsolete_weather_images "$source_env" "$target_env"
  proof_before="$temporary/proof.before"
  capacity_before="$temporary/capacity.before"
  dashboard_package_receipt "$proof_before" web-release-proof \
    "$target_release" "$expected_core"
  require_dashboard_publication_proof "$proof_before" "$target_release" "$expected_core"
  dashboard_package_receipt "$capacity_before" web-release-capacity \
    "$source_image" "$target_image"
  require_dashboard_capacity_receipt "$capacity_before" "$source_image" "$target_image"
  WEATHER_ENV_FILE=$target_env compose pull web
  capacity_after="$temporary/capacity.after"
  dashboard_package_receipt "$capacity_after" web-release-capacity \
    "$source_image" "$target_image"
  require_dashboard_capacity_receipt "$capacity_after" "$source_image" "$target_image"
  original_current=$(read_optional_release_state "$state_dir/current-web-release")
  original_previous=$(read_optional_release_state "$state_dir/previous-web-release")
  original_current=${original_current:-none}
  original_previous=${original_previous:-none}
  proof_hash=$(sha256sum "$proof_before" | awk '{print $1}')
  write_dashboard_release_journal release "$expected_core" "$expected_source" \
    "$target_release" "$original_current" "$original_previous" "$source_env" \
    "$target_env" "$source_image" "$target_image" "$proof_hash" "$stable_snapshot_before"
  started=true
  start_dashboard_web "$target_env" || die "dashboard web start failed"
  verify_dashboard_web_runtime "$target_env" "$target_release" "$target_image"
  proof_after="$temporary/proof.after"
  dashboard_package_receipt "$proof_after" web-release-proof \
    "$target_release" "$expected_core"
  require_dashboard_publication_proof "$proof_after" "$target_release" "$expected_core"
  [[ "$(sha256sum "$proof_after" | awk '{print $1}')" == "$proof_hash" ]] ||
    die "dashboard publication proof changed during deployment"
  require_dashboard_core_cas "$expected_core"
  core_snapshot_after=$(dashboard_core_snapshot_sha256 \
    "$core_env" "$temporary/core.after")
  stable_snapshot_after=$(dashboard_core_snapshot_sha256 \
    "$core_env" "$temporary/core-stable.after" false)
  [[ "$core_snapshot_after" == "$core_snapshot_before" ]] ||
    die "dashboard release changed the core runtime"
  [[ "$stable_snapshot_after" == "$stable_snapshot_before" ]] ||
    die "dashboard release changed stable core authority"
  [[ "$(read_optional_release_state "$state_dir/current-web-release")" == \
    "${original_current/none/}" ]] || die "dashboard source marker changed during deployment"
  record_dashboard_release_success "$target_release" "$expected_source" "$expected_core"
  remove_dashboard_state "$dashboard_release_journal"
  started=false
  trap - EXIT

  cleanup_obsolete_weather_images "$source_env" "$target_env"
  rm -rf -- "$temporary"
  printf 'Dashboard web release %s is active over core %s.\n' \
    "$target_release" "$expected_core"
)

# roll the web override back without changing core markers or services
dashboard_rollback() (
  local target_mode=${1:-previous}
  local rollback_core rollback_source rollback_target rollback_source_env rollback_target_env
  local rollback_source_image rollback_target_image
  local rollback_original_current rollback_original_previous proof_hash
  local core_snapshot_before core_snapshot_after
  local stable_snapshot_before stable_snapshot_after
  local status=0 started=false
  local temporary proof_before proof_after
  temporary=$(mktemp -d "${TMPDIR:-/tmp}/weather-dashboard-rollback.XXXXXX")

  # compensate every post-journal failure with the exact incumbent web
  # shellcheck disable=SC2317,SC2329
  cleanup_dashboard_rollback() {
    status=$?
    if [[ "$started" == true ]]; then
      if ! start_dashboard_web "$rollback_source_env" ||
        ! verify_dashboard_web_runtime \
          "$rollback_source_env" "$rollback_source" "$rollback_source_image" ||
        ! restore_dashboard_marker_snapshot \
          "$rollback_original_current" "$rollback_original_previous"; then
        status=1
      else
        remove_dashboard_state "$dashboard_release_journal" || status=1
      fi
    fi
    rm -rf -- "$temporary"
    trap - EXIT
    exit "$status"
  }
  trap cleanup_dashboard_rollback EXIT
  [[ "$target_mode" == previous || "$target_mode" == core ]] ||
    die "dashboard rollback target mode is invalid"
  [[ ! -e "$dashboard_release_journal" && ! -L "$dashboard_release_journal" ]] ||
    die "dashboard release recovery is required"
  rollback_core=$(read_release_state "$state_dir/current-release")
  require_dashboard_core_cas "$rollback_core"
  rollback_source=$(read_release_state "$state_dir/current-web-release")
  rollback_original_previous=$(read_release_state "$state_dir/previous-web-release")
  rollback_target=$rollback_original_previous

  # provide a proof-bound escape from any retained web rollback chain
  if [[ "$target_mode" == core ]]; then
    rollback_target=$rollback_core
  fi
  [[ "$rollback_source" != "$rollback_target" ]] ||
    die "previous dashboard release matches the active web"
  rollback_source_env=$(dashboard_source_environment "$rollback_core" "$rollback_source")
  rollback_source_image=$(env_value "$rollback_source_env" WEATHER_WEB_IMAGE)
  verify_dashboard_web_runtime \
    "$rollback_source_env" "$rollback_source" "$rollback_source_image"
  core_snapshot_before=$(dashboard_core_snapshot_sha256 \
    "$(release_env "$rollback_core")" "$temporary/core.before")
  stable_snapshot_before=$(dashboard_core_snapshot_sha256 \
    "$(release_env "$rollback_core")" "$temporary/core-stable.before" false)

  # select either the unchanged core or one retained web override
  if [[ "$rollback_target" == "$rollback_core" ]]; then
    rollback_target_env=$(release_env "$rollback_core")
  else
    rollback_target_env=$(dashboard_release_env "$rollback_target")
    validate_dashboard_release_env \
      "$rollback_target_env" "$rollback_target" "$rollback_core"
  fi
  rollback_target_image=$(env_value "$rollback_target_env" WEATHER_WEB_IMAGE)
  docker image inspect "$rollback_target_image" >/dev/null ||
    die "previous dashboard web image is unavailable"
  proof_before="$temporary/proof.before"
  dashboard_package_receipt \
    "$proof_before" web-release-proof "$rollback_target" "$rollback_core"
  require_dashboard_publication_proof \
    "$proof_before" "$rollback_target" "$rollback_core"
  rollback_original_current=$rollback_source
  proof_hash=$(sha256sum "$proof_before" | awk '{print $1}')
  write_dashboard_release_journal \
    rollback "$rollback_core" "$rollback_source" "$rollback_target" \
    "$rollback_original_current" "$rollback_original_previous" \
    "$rollback_source_env" "$rollback_target_env" \
    "$rollback_source_image" "$rollback_target_image" \
    "$proof_hash" "$stable_snapshot_before"
  started=true
  start_dashboard_web "$rollback_target_env" ||
    die "dashboard rollback web start failed"
  verify_dashboard_web_runtime \
    "$rollback_target_env" "$rollback_target" "$rollback_target_image"
  proof_after="$temporary/proof.after"
  dashboard_package_receipt \
    "$proof_after" web-release-proof "$rollback_target" "$rollback_core"
  require_dashboard_publication_proof \
    "$proof_after" "$rollback_target" "$rollback_core"
  [[ "$(sha256sum "$proof_after" | awk '{print $1}')" == "$proof_hash" ]] ||
    die "dashboard rollback publication proof changed"
  require_dashboard_core_cas "$rollback_core"
  core_snapshot_after=$(dashboard_core_snapshot_sha256 \
    "$(release_env "$rollback_core")" "$temporary/core.after")
  stable_snapshot_after=$(dashboard_core_snapshot_sha256 \
    "$(release_env "$rollback_core")" "$temporary/core-stable.after" false)
  [[ "$core_snapshot_after" == "$core_snapshot_before" ]] ||
    die "dashboard rollback changed the core runtime"
  [[ "$stable_snapshot_after" == "$stable_snapshot_before" ]] ||
    die "dashboard rollback changed stable core authority"
  [[ "$(read_release_state "$state_dir/current-web-release")" == "$rollback_source" ]] ||
    die "dashboard source marker changed during rollback"
  record_dashboard_release_success \
    "$rollback_target" "$rollback_source" "$rollback_core"
  remove_dashboard_state "$dashboard_release_journal"
  started=false
  trap - EXIT

  # release the retired override only after returning to the core web
  if [[ "$rollback_target" == "$rollback_core" ]]; then
    cleanup_obsolete_weather_images "$rollback_target_env" "$rollback_target_env"
  else
    cleanup_obsolete_weather_images "$rollback_source_env" "$rollback_target_env"
  fi
  rm -rf -- "$temporary"
  printf 'Dashboard web release %s is active after web-only rollback.\n' \
    "$rollback_target"
)

# require the inherited family command to retain the shared release lock
require_adjustment_family_release_lock() {
  local descriptor=${WEATHER_RELEASE_LOCK_FD:-}
  [[ "$descriptor" =~ ^[1-9][0-9]{0,2}$ && -e "/proc/self/fd/$descriptor" ]] ||
    die "adjustment family release lock is unavailable"
  [[ "$(stat -Lc '%d:%i' "/proc/self/fd/$descriptor")" == \
    "$(stat -c '%d:%i' "$adjustment_family_deploy_state")" ]] ||
    die "adjustment family release lock identity differs"
}

# create only an absent private transaction root
ensure_adjustment_family_private_state_root() {
  local root=$1

  # create only a genuinely absent leaf beneath the already trusted state directory
  if [[ ! -e "$root" && ! -L "$root" ]]; then
    install -d -o 0 -g 0 -m 0700 "$root"
  fi

  # refuse existing ownership or mode drift without repairing or adopting it
  [[ -d "$root" && ! -L "$root" &&
    "$(realpath -e "$root")" == "$root" &&
    "$(stat -c '%u:%g:%a' "$root")" == "0:0:700" ]] ||
    die "adjustment family transaction root is unsafe"
}

# verify the installed state roots match the fixed privileged paths
require_adjustment_family_fixed_roots() {
  local ancestor mode

  # reject linked, foreign or other-writable ancestors in the installed tree
  for ancestor in /opt /opt/weather /opt/weather/current /opt/weather/current/deploy \
    "$adjustment_family_deploy_state" "$adjustment_family_releases"; do
    [[ -d "$ancestor" && ! -L "$ancestor" && "$(realpath -e "$ancestor")" == "$ancestor" &&
      "$(stat -c '%u:%g' "$ancestor")" == "0:0" ]] ||
      die "adjustment family deployment ancestor is unsafe: $ancestor"
    mode=$(stat -c '%a' "$ancestor")
    (( (8#$mode & 0002) == 0 )) ||
      die "adjustment family deployment ancestor is other-writable: $ancestor"
  done
  [[ "$(stat -c '%a' "$adjustment_family_deploy_state")" == 775 &&
    "$(stat -c '%a' "$adjustment_family_releases")" == 775 ]] ||
    die "adjustment family deployment directory mode differs"
  [[ "$(stat -c '%d:%i' "$state_dir")" == \
    "$(stat -c '%d:%i' "$adjustment_family_deploy_state")" ]] ||
    die "adjustment family deployment state root differs"
  [[ "$(stat -c '%d:%i' "$releases_dir")" == \
    "$(stat -c '%d:%i' "$adjustment_family_releases")" ]] ||
    die "adjustment family release root differs"
  ensure_adjustment_family_private_state_root "$adjustment_family_state_root"
}

# compare the live release and exact operator settings before preparation work
require_adjustment_family_source_cas() {
  local expected_release=$1
  local expected_settings_sha256=$2
  local owner size
  [[ "$(read_release_state "$state_dir/current-release")" == "$expected_release" ]] ||
    die "adjustment family source release differs before preparation"
  [[ -f "$adjustment_family_settings" && ! -L "$adjustment_family_settings" &&
    "$(realpath -e "$adjustment_family_settings")" == "$adjustment_family_settings" &&
    "$(stat -c '%a:%h' "$adjustment_family_settings")" == "600:1" ]] ||
    die "adjustment family settings file is unsafe"
  owner=$(stat -c '%u:%g' "$adjustment_family_settings")
  [[ "$owner" == 0:0 || "$owner" == 10002:10002 ]] ||
    die "adjustment family settings owner differs"
  size=$(stat -c '%s' "$adjustment_family_settings")
  [[ "$size" =~ ^[1-9][0-9]{0,3}$ && "$size" -le 4096 ]] ||
    die "adjustment family settings size is invalid"
  [[ "$(sha256sum "$adjustment_family_settings" | awk '{print $1}')" == \
    "$expected_settings_sha256" ]] ||
    die "adjustment family settings changed before preparation"
}

# render one family release from exact current infrastructure settings
render_adjustment_family_release_env() {
  local release=$1
  local source_env=$2
  local target server_image web_image temporary key
  validate_release "$release"
  require_file "$source_env"
  target=$(release_env "$release")
  server_image=$(resolve_arm64_image "$weather_server_image_repository:$release")
  web_image=$(resolve_arm64_image "$weather_web_image_repository:$release")

  # reuse only an environment with exact resolved images and retained infrastructure
  if [[ -e "$target" || -L "$target" ]]; then
    validate_release_env "$target" "$release"
    [[ "$(env_value "$target" WEATHER_SERVER_IMAGE)" == "$server_image" &&
      "$(env_value "$target" WEATHER_WEB_IMAGE)" == "$web_image" ]] ||
      die "adjustment family release images differ"
    for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
      WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
      WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH; do
      [[ "$(env_value "$target" "$key")" == "$(env_value "$source_env" "$key")" ]] ||
        die "adjustment family release infrastructure differs: $key"
    done
    printf '%s\n' "$target"
    return
  fi
  temporary=$(mktemp "$releases_dir/.${release}.XXXXXX.env.partial")
  trap 'rm -f -- "$temporary"' RETURN
  write_release_env "$source_env" "$temporary" "$release" "$server_image" "$web_image" \
    "$(env_value "$source_env" POSTGRES_IMAGE)" \
    "$(env_value "$source_env" CLOUDFLARED_IMAGE)"
  validate_release_env "$temporary" "$release"
  mv "$temporary" "$target"
  trap - RETURN
  printf '%s\n' "$target"
}

# prepare both immutable releases and one pre-pull literal capacity receipt
prepare_adjustment_family_release_pair() {
  local target_release=$1
  local compensating_release=$2
  local source_release=$3
  local family=$4
  local action_sha256=$5
  local source_env target_env compensating_env capacity temporary status image
  local -a images
  require_adjustment_family_fixed_roots
  source_env=$(release_env "$source_release")
  validate_release_env "$source_env" "$source_release"
  require_control_plane_compatibility "$source_env"
  target_env=$(render_adjustment_family_release_env "$target_release" "$source_env")
  compensating_env=$(render_adjustment_family_release_env "$compensating_release" "$source_env")
  capacity="$adjustment_family_state_root/capacity-sha256-${action_sha256}.json"
  temporary=$(mktemp "$adjustment_family_state_root/.capacity.XXXXXX.tmp")
  trap 'rm -f -- "$temporary"' RETURN
  status=0
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" family-release-capacity \
    "$action_sha256" "$family" "$source_release" \
    "$(env_value "$source_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$source_env" WEATHER_WEB_IMAGE)" \
    "$(env_value "$target_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$target_env" WEATHER_WEB_IMAGE)" \
    "$(env_value "$compensating_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$compensating_env" WEATHER_WEB_IMAGE)" >"$temporary" || status=$?
  chmod 600 "$temporary"

  # preserve the refusal receipt while denying every blocked pull
  if ((status != 0)); then
    cat "$temporary" >&2
    die "adjustment family release capacity is blocked"
  fi
  mv "$temporary" "$capacity"
  trap - RETURN
  mapfile -t images < <(printf '%s\n' \
    "$(env_value "$target_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$target_env" WEATHER_WEB_IMAGE)" \
    "$(env_value "$compensating_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$compensating_env" WEATHER_WEB_IMAGE)" | LC_ALL=C sort -u)

  # pull only the four literal application references covered by capacity
  for image in "${images[@]}"; do
    validate_image_reference "$image"
    docker image pull "$image" >/dev/null
  done
}

# publish one family-only release marker without touching schema state
record_adjustment_family_release_success() {
  local target=$1
  local baseline=$2
  write_private_state "$state_dir/previous-release" "$baseline"
  write_active_symlink "$target"
  write_private_state "$state_dir/current-release" "$target"
}

# switch only application images under the inherited family transaction lock
apply_adjustment_family_release_unlocked() {
  local target=$1
  local expected_current=$2
  local alternate_current=$3
  local current target_env current_env key
  require_adjustment_family_release_lock
  current=$(read_release_state "$state_dir/current-release")

  # accept only an already committed exact-target retry
  if [[ "$current" == "$target" ]]; then
    target_env=$(release_env "$target")
    validate_release_env "$target_env" "$target"
    require_control_plane_compatibility "$target_env"
    [[ -L "$state_dir/active.env" &&
      "$(readlink "$state_dir/active.env")" == "../releases/$target.env" ]] ||
      die "adjustment family active release link differs"
    return
  fi
  [[ "$current" == "$expected_current" || "$current" == "$alternate_current" ]] ||
    die "adjustment family current release differs"
  target_env=$(release_env "$target")
  current_env=$(release_env "$current")
  validate_release_env "$target_env" "$target"
  validate_release_env "$current_env" "$current"
  require_control_plane_compatibility "$target_env"
  require_control_plane_compatibility "$current_env"

  # family releases retain every database, infrastructure and operator setting
  for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH; do
    [[ "$(env_value "$target_env" "$key")" == "$(env_value "$current_env" "$key")" ]] ||
      die "adjustment family live infrastructure differs: $key"
  done
  require_deployment_secrets
  restore_images "$target_env" || die "adjustment family image switch failed"
  record_adjustment_family_release_success "$target" "$current"
}

# prepare one direct release without clone-based staging
prepare_yolo_release() {
  local release=$1
  local source_env=$2
  local target server_source web_source server_image web_image postgres_image
  local cloudflared_image temporary image
  local -a images
  target=$(release_env "$release")

  # reuse an exact previously rendered release
  if [[ -e "$target" || -L "$target" ]]; then
    validate_release_env "$target" "$release"
    require_control_plane_compatibility "$target"
    cleanup_obsolete_weather_images "$source_env" "$target"
    require_image_pull_capacity_floor
    require_v13_resource_gate
    require_literal_adjustment_release_capacity "$source_env" "$target"
    WEATHER_ENV_FILE=$target compose pull >&2
    require_literal_adjustment_release_capacity "$source_env" "$target"
    printf '%s\n' "$target"
    return
  fi

  server_source="$(image_repository "$(env_value "$source_env" WEATHER_SERVER_IMAGE)"):$release"
  web_source="$(image_repository "$(env_value "$source_env" WEATHER_WEB_IMAGE)"):$release"
  server_image=$(resolve_arm64_image "$server_source")
  web_image=$(resolve_arm64_image "$web_source")
  postgres_image=$(resolve_arm64_image "$(env_value "$source_env" POSTGRES_IMAGE)")
  cloudflared_image=$(resolve_arm64_image "$(env_value "$source_env" CLOUDFLARED_IMAGE)")
  temporary=$(mktemp "$releases_dir/.${release}.XXXXXX.env.partial")

  # remove an interrupted render
  trap 'rm -f "$temporary"' EXIT
  write_release_env "$source_env" "$temporary" "$release" \
    "$server_image" "$web_image" "$postgres_image" "$cloudflared_image"
  WEATHER_ENV_FILE=$temporary compose config --quiet
  mapfile -t images < <(WEATHER_ENV_FILE=$temporary compose config --images | sort -u)
  ((${#images[@]} == 4)) || die "release must contain exactly four images"

  # reject any tag-only rendered image
  for image in "${images[@]}"; do
    validate_image_reference "$image"
  done

  cleanup_obsolete_weather_images "$source_env" "$temporary"
  require_image_pull_capacity_floor
  require_v13_resource_gate
  require_literal_adjustment_release_capacity "$source_env" "$temporary"
  WEATHER_ENV_FILE=$temporary compose pull >&2
  require_literal_adjustment_release_capacity "$source_env" "$temporary"
  mv "$temporary" "$target"
  trap - EXIT
  printf '%s\n' "$target"
}

# apply one source-preserving direct release without deployment-time backup
yolo_release() (
  local release=$1
  local source_env=$2
  local target current current_env yolo_authorization_path temporary_authorization
  local actual_history expected_history source_history restored_history schema_after_restore key
  local previous_before=
  local previous_existed=false
  local active_mutation_started=false
  local completed=false
  current=$(read_optional_release_state "$state_dir/current-release")

  # restore the exact retained source after every active-database mutation failure
  # shellcheck disable=SC2317,SC2329
  restore_failed_yolo() {
    local status=$?
    local recovery_status=0
    trap - EXIT
    set +e

    # remove only an unpublished compatibility receipt
    if [[ -n "${temporary_authorization:-}" ]]; then
      rm -f -- "$temporary_authorization"
    fi

    # compensate only after the live database or service transaction began
    if [[ "$completed" != true && "$active_mutation_started" == true ]]; then
      printf 'Direct deployment failed; restoring Weather release %s...\n' \
        "$current" >&2
      if restore_images "$current_env" "$current" "$release"; then
        # reconcile durable state before the independent capacity diagnostic
        expected_history=$(env_value \
          "$yolo_authorization_path" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256) ||
          recovery_status=1
        restored_history=$(migration_history_sha256 \
          "$current_env" "$(env_value "$current_env" WEATHER_DATABASE_NAME)") ||
          recovery_status=1
        schema_after_restore=

        # accept only one of the two pre-proven complete migration ledgers
        if [[ "$restored_history" == "$expected_history" ]]; then
          schema_after_restore=$release
        elif [[ "$restored_history" == "$source_history" ]]; then
          schema_after_restore=$current
        else
          recovery_status=1
        fi

        # publish a schema marker only for an exact recognized ledger
        if [[ -n "$schema_after_restore" ]]; then
          write_private_state "$state_dir/schema-release" "$schema_after_restore" ||
            recovery_status=1
        fi
        write_active_symlink "$current" || recovery_status=1
        write_private_state "$state_dir/current-release" "$current" || recovery_status=1

        # restore the pre-transaction rollback marker after a partial commit
        if [[ "$previous_existed" == true ]]; then
          write_private_state "$state_dir/previous-release" "$previous_before" ||
            recovery_status=1
        else
          rm -f -- "$state_dir/previous-release" || recovery_status=1
        fi

        # report compensation capacity only after source state is durable
        require_literal_adjustment_release_capacity "$source_env" "$target" ||
          recovery_status=1
      else
        recovery_status=1
      fi

      # never report an original or compensation failure as a successful release
      if ((recovery_status != 0)); then
        printf 'Direct deployment and exact source restoration both failed.\n' >&2
        exit 1
      fi
    fi
    exit "$status"
  }
  trap restore_failed_yolo EXIT

  # require one exact retained predecessor before rendering or pulling images
  [[ "$current" == "$maintenance_previous_release" ]] ||
    die "control plane v13 requires the exact retained source release"

  # freeze the rollback marker before record_release_success can replace it
  if [[ -e "$state_dir/previous-release" || -L "$state_dir/previous-release" ]]; then
    previous_before=$(read_release_state "$state_dir/previous-release")
    previous_existed=true
  fi
  # shellcheck disable=SC2030
  current_env=$(release_env "$current")
  validate_release_env "$current_env" "$current"
  validate_release_env "$source_env" "$current"
  require_control_plane_compatibility "$current_env"
  require_control_plane_compatibility "$source_env"

  # prevent an explicit source file from changing any retained release setting
  for key in WEATHER_RELEASE WEATHER_SERVER_IMAGE WEATHER_WEB_IMAGE POSTGRES_IMAGE \
    CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH \
    WEATHER_CONTROL_PLANE_SHA256 WEATHER_CONTROL_PLANE_VERSION; do
    [[ "$(env_value "$source_env" "$key")" == "$(env_value "$current_env" "$key")" ]] ||
      die "direct deployment source differs from the retained release: $key"
  done

  target=$(prepare_yolo_release "$release" "$source_env")
  require_control_plane_compatibility "$target"
  require_deployment_secrets
  yolo_authorization_path=$(migration_authorization "$release")
  temporary_authorization=$(mktemp \
    "$releases_dir/.${release}.XXXXXX.migration-authorization.partial")
  verify_fixed_v13_source_compatibility \
    "$target" "$source_env" "$temporary_authorization"

  # reuse only byte-identical previously proven compatibility authorization
  if [[ -e "$yolo_authorization_path" || -L "$yolo_authorization_path" ]]; then
    validate_migration_authorization \
      "$yolo_authorization_path" "$current" "$release"
    cmp -s "$temporary_authorization" "$yolo_authorization_path" ||
      die "existing fixed v13 migration authorization differs"
    rm -f -- "$temporary_authorization"
    temporary_authorization=
  else
    publish_migration_authorization \
      "$temporary_authorization" "$yolo_authorization_path"
    temporary_authorization=
  fi

  # bind compensation to the exact complete source ledger observed before mutation
  source_history=$(migration_history_sha256 \
    "$current_env" "$(env_value "$current_env" WEATHER_DATABASE_NAME)")
  [[ "$source_history" =~ ^[a-f0-9]{64}$ ]] ||
    die "fixed v13 source migration ledger is invalid"

  # remeasure fresh resources and literal images at the last reversible boundary
  require_v13_resource_gate || die "fixed v13 resource gate failed before activation"
  require_literal_adjustment_release_capacity "$source_env" "$target" ||
    die "fixed v13 literal capacity failed before activation"

  printf 'Applying release %s directly...\n' "$release"
  active_mutation_started=true
  start_postgres "$target" || die "fixed v13 PostgreSQL restart failed"
  WEATHER_ENV_FILE=$target compose run --rm migration ||
    die "fixed v13 active migration failed"
  actual_history=$(migration_history_sha256 \
    "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)")
  expected_history=$(env_value \
    "$yolo_authorization_path" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)
  [[ "$actual_history" == "$expected_history" ]] ||
    die "active migration ledger differs from fixed v13 compatibility proof"
  write_private_state "$state_dir/schema-release" "$release" ||
    die "fixed v13 schema marker publication failed"
  apply_runtime_database_acl "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)" ||
    die "fixed v13 runtime ACL application failed"
  verify_runtime_database_acl "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)" ||
    die "fixed v13 runtime ACL verification failed"
  start_exact_release "$target" || die "fixed v13 target release health failed"
  require_literal_adjustment_release_capacity "$source_env" "$target" ||
    die "fixed v13 idle literal capacity failed"
  record_release_success "$release" "$current" ||
    die "fixed v13 release state publication failed"
  completed=true
  trap - EXIT
  printf 'Release %s is active after direct deployment.\n' "$release"
)

# initialize only an exact empty root-owned catalog for the inactive code handoff
require_inert_v14_empty_catalog() {
  local catalog="$state_dir/adjustment-candidate-catalog.json"
  local owner group mode links
  require_adjustment_family_fixed_roots
  # atomically create the fixed read-only bind source without an application-owned directory
  if [[ ! -e "$catalog" && ! -L "$catalog" ]]; then
    local temporary
    temporary=$(mktemp "$state_dir/.adjustment-candidate-catalog.XXXXXX")
    printf '%s\n' '{"contractVersion":"adjustment-installed-candidate-catalog/v1","entries":[]}' >"$temporary"
    chmod 644 "$temporary"
    chown 0:0 "$temporary"
    sync -f "$temporary"
    ln "$temporary" "$catalog" || { rm -f -- "$temporary"; die "inactive v14 catalog publication collided"; }
    rm -f -- "$temporary"
    sync -f "$state_dir"
  fi
  [[ -f "$catalog" && ! -L "$catalog" ]] || die "inactive v14 catalog must be a regular file"
  read -r owner group mode links < <(stat --format='%u %g %a %h' "$catalog")
  [[ "$owner" == 0 && "$group" == 0 && "$mode" == 644 && "$links" == 1 ]] ||
    die "inactive v14 catalog ownership differs"
  cmp -s "$catalog" <(printf '%s\n' '{"contractVersion":"adjustment-installed-candidate-catalog/v1","entries":[]}') ||
    die "inactive v14 catalog is not the exact inert empty baseline"
}

# prepare only the fixed compatibility bridge without schema work
prepare_v14_compatibility_bridge_release() {
  local release=$1
  local source_env=$2
  local target server_source web_source server_image web_image postgres_image
  local cloudflared_image temporary image
  local -a images
  [[ "$release" == "$v14_compatibility_bridge_release" ]] ||
    die "v14 compatibility bridge release differs"
  target=$(release_env "$release")

  # reuse only the exact published bridge environment
  if [[ -e "$target" || -L "$target" ]]; then
    validate_release_env "$target" "$release"
    require_control_plane_compatibility "$target"
    require_v14_compatibility_bridge_release_pair "$target" "$source_env"
    cleanup_obsolete_weather_images "$source_env" "$target"
    require_image_pull_capacity_floor
    require_normal_release_resource_gate
    require_literal_inert_v14_release_capacity "$source_env" "$target"
    WEATHER_ENV_FILE=$target compose pull >&2
    require_literal_inert_v14_release_capacity "$source_env" "$target"
    printf '%s\n' "$target"
    return
  fi

  server_source="$(image_repository "$(env_value "$source_env" WEATHER_SERVER_IMAGE)"):$release"
  web_source="$(image_repository "$(env_value "$source_env" WEATHER_WEB_IMAGE)"):$release"
  server_image=$(resolve_arm64_image "$server_source")
  web_image=$(resolve_arm64_image "$web_source")
  postgres_image=$(resolve_arm64_image "$(env_value "$source_env" POSTGRES_IMAGE)")
  cloudflared_image=$(resolve_arm64_image "$(env_value "$source_env" CLOUDFLARED_IMAGE)")
  temporary=$(mktemp "$releases_dir/.${release}.XXXXXX.env.partial")

  # remove an interrupted render
  trap 'rm -f "$temporary"' EXIT
  write_release_env "$source_env" "$temporary" "$release" \
    "$server_image" "$web_image" "$postgres_image" "$cloudflared_image"
  require_v14_compatibility_bridge_release_pair "$temporary" "$source_env"
  WEATHER_ENV_FILE=$temporary compose config --quiet
  mapfile -t images < <(WEATHER_ENV_FILE=$temporary compose config --images | sort -u)
  ((${#images[@]} == 4)) || die "bridge release must contain exactly four images"

  # reject every tag-only image
  for image in "${images[@]}"; do
    validate_image_reference "$image"
  done

  cleanup_obsolete_weather_images "$source_env" "$temporary"
  require_image_pull_capacity_floor
  require_normal_release_resource_gate
  require_literal_inert_v14_release_capacity "$source_env" "$temporary"
  WEATHER_ENV_FILE=$temporary compose pull >&2
  require_literal_inert_v14_release_capacity "$source_env" "$temporary"
  mv "$temporary" "$target"
  trap - EXIT
  printf '%s\n' "$target"
}

# apply the compatibility-only bridge on the unchanged 0018 database
v14_compatibility_bridge_release() (
  local release=$1
  local source_env target current schema previous_before=absent
  local source_history actual_history bridge_settings_snapshot bridge_settings_inode
  local bridge_settings_sha256 git_proof bridge_target_commit committed=false
  local active_mutation_started=false
  local recovery_status=0

  # restore exact source-one state after every precommit mutation failure
  # shellcheck disable=SC2317,SC2329
  restore_failed_v14_compatibility_bridge() {
    local status=$?
    trap - EXIT
    # leave the successful current-marker commit as the final operation
    if [[ "$status" == 0 && "$committed" == true ]]; then
      exit 0
    fi
    set +e
    # compensate only after durable intent and runtime mutation
    if [[ "$active_mutation_started" == true ]]; then
      recover_v14_compatibility_bridge || recovery_status=1
    fi
    # never hide a failed source restoration
    if ((recovery_status != 0)); then
      printf 'V14 compatibility bridge and exact source restoration both failed.\n' >&2
      exit 1
    fi
    exit "$status"
  }
  trap restore_failed_v14_compatibility_bridge EXIT
  [[ "$release" == "$v14_compatibility_bridge_release" ]] ||
    die "v14 compatibility bridge release differs"

  # finish or compensate only the one durable prior attempt
  if [[ -e "$v14_compatibility_bridge_state" || -L "$v14_compatibility_bridge_state" ]]; then
    current=$(read_release_state "$state_dir/current-release")
    schema=$(read_optional_release_state "$state_dir/schema-release")
    case "$current:$schema" in
      "$recurring_previous_release:$recurring_previous_release"|\
      "$recurring_previous_release:$v14_compatibility_bridge_release")
        # compensate only the source-one bridge transaction
        active_mutation_started=true
        recover_v14_compatibility_bridge || die "interrupted v14 compatibility bridge recovery failed"
        active_mutation_started=false
        ;;
      "$v14_compatibility_bridge_release:$v14_compatibility_bridge_release")
        # preserve an already committed byte-identical bridge
        require_committed_v14_compatibility_bridge
        committed=true
        return 0
        ;;
      *)
        die "v14 compatibility bridge cannot rewind an advanced release state"
        ;;
    esac
  fi

  current=$(read_release_state "$state_dir/current-release")
  schema=$(read_release_state "$state_dir/schema-release")
  [[ "$current" == "$recurring_previous_release" &&
    "$schema" == "$recurring_previous_release" ]] ||
    die "v14 compatibility bridge requires exact source runtime and schema markers"
  require_v14_bridge_active_release "$recurring_previous_release"
  source_env=$(release_env "$recurring_previous_release")
  validate_release_env "$source_env" "$recurring_previous_release"
  require_control_plane_compatibility "$source_env"
  require_deployment_secrets
  # preserve the original rollback marker for compensation
  if [[ -e "$state_dir/previous-release" || -L "$state_dir/previous-release" ]]; then
    previous_before=$(read_release_state "$state_dir/previous-release")
  fi

  git_proof=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    verify-v14-compatibility-bridge-release "$release") ||
    die "v14 compatibility bridge exact-commit CI proof failed"
  bridge_target_commit=$(node --input-type=module -e '
const proof = JSON.parse(process.argv[1]);
// require the independently frozen bridge commit
if (proof.targetCommit !== process.argv[2]) throw new Error("bridge commit differs");
process.stdout.write(proof.targetCommit);
' "$git_proof" "$v14_compatibility_bridge_commit")
  [[ "$bridge_target_commit" == "$v14_compatibility_bridge_commit" ]] ||
    die "v14 compatibility bridge commit differs"
  target=$(prepare_v14_compatibility_bridge_release "$release" "$source_env")
  require_v14_compatibility_bridge_release_pair "$target" "$source_env"
  require_v14_compatibility_bridge_migration_ledger \
    "$source_env" "$(env_value "$source_env" WEATHER_DATABASE_NAME)"
  source_history=$(migration_history_sha256 \
    "$source_env" "$(env_value "$source_env" WEATHER_DATABASE_NAME)")
  [[ "$source_history" =~ ^[a-f0-9]{64}$ ]] ||
    die "v14 compatibility bridge source ledger hash is invalid"
  bridge_settings_snapshot=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    inert-v14-settings-snapshot)
  read -r bridge_settings_inode bridge_settings_sha256 <<<"$bridge_settings_snapshot"
  [[ "$bridge_settings_inode" =~ ^[0-9]+:[0-9]+$ &&
    "$bridge_settings_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "v14 compatibility bridge settings snapshot is invalid"

  # remeasure every admission gate immediately before mutation
  require_image_pull_capacity_floor
  require_normal_release_resource_gate
  require_literal_inert_v14_release_capacity "$source_env" "$target"
  publish_v14_compatibility_bridge_state "$source_env" "$target" "$source_history" \
    "$bridge_settings_inode" "$bridge_settings_sha256" "$previous_before"
  active_mutation_started=true
  start_v14_compatibility_bridge_runtime "$target" ||
    die "v14 compatibility bridge target health failed"

  # prove the database, settings, controls and capacity remained unchanged
  require_v14_compatibility_bridge_release_pair "$target" "$source_env"
  require_v14_compatibility_bridge_migration_ledger \
    "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)"
  actual_history=$(migration_history_sha256 \
    "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)")
  [[ "$actual_history" == "$source_history" ]] ||
    die "v14 compatibility bridge changed the 0018 ledger"
  [[ "$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
      inert-v14-settings-snapshot)" == "$bridge_settings_snapshot" ]] ||
    die "v14 compatibility bridge settings changed"
  [[ "$(read_release_state "$state_dir/current-release")" == "$recurring_previous_release" &&
    "$(read_release_state "$state_dir/schema-release")" == "$recurring_previous_release" ]] ||
    die "v14 compatibility bridge source markers changed before commit"
  require_image_pull_capacity_floor
  require_normal_release_resource_gate
  require_literal_inert_v14_release_capacity "$source_env" "$target"

  # publish every reversible marker before the final current-release commit
  write_private_state "$state_dir/previous-release" "$recurring_previous_release"
  write_private_state "$state_dir/schema-release" "$release"
  write_active_symlink "$release"
  committed=true
  write_private_state "$state_dir/current-release" "$release"
)

# prepare one direct release without clone-based staging
prepare_inert_v14_release() {
  local release=$1
  local source_env=$2
  local target server_source web_source server_image web_image postgres_image
  local cloudflared_image temporary image
  local -a images
  target=$(release_env "$release")

  # reuse an exact previously rendered release
  if [[ -e "$target" || -L "$target" ]]; then
    validate_release_env "$target" "$release"
    require_control_plane_compatibility "$target"
    require_fixed_v14_source_identity "$target" "$source_env"
    cleanup_obsolete_weather_images "$source_env" "$target"
    require_image_pull_capacity_floor
    require_normal_release_resource_gate
    require_literal_full_v14_release_capacity "$source_env" "$target"
    WEATHER_ENV_FILE=$target compose pull >&2
    require_literal_full_v14_release_capacity "$source_env" "$target"
    printf '%s\n' "$target"
    return
  fi

  server_source="$(image_repository "$(env_value "$source_env" WEATHER_SERVER_IMAGE)"):$release"
  web_source="$(image_repository "$(env_value "$source_env" WEATHER_WEB_IMAGE)"):$release"
  server_image=$(resolve_arm64_image "$server_source")
  web_image=$(resolve_arm64_image "$web_source")
  postgres_image=$(resolve_arm64_image "$(env_value "$source_env" POSTGRES_IMAGE)")
  cloudflared_image=$(resolve_arm64_image "$(env_value "$source_env" CLOUDFLARED_IMAGE)")
  temporary=$(mktemp "$releases_dir/.${release}.XXXXXX.env.partial")

  # remove an interrupted render
  trap 'rm -f "$temporary"' EXIT
  write_release_env "$source_env" "$temporary" "$release" \
    "$server_image" "$web_image" "$postgres_image" "$cloudflared_image"
  require_fixed_v14_source_identity "$temporary" "$source_env"
  WEATHER_ENV_FILE=$temporary compose config --quiet
  mapfile -t images < <(WEATHER_ENV_FILE=$temporary compose config --images | sort -u)
  ((${#images[@]} == 4)) || die "release must contain exactly four images"

  # reject any tag-only rendered image
  for image in "${images[@]}"; do
    validate_image_reference "$image"
  done

  cleanup_obsolete_weather_images "$source_env" "$temporary"
  require_image_pull_capacity_floor
  require_normal_release_resource_gate
  require_literal_full_v14_release_capacity "$source_env" "$temporary"
  WEATHER_ENV_FILE=$temporary compose pull >&2
  require_literal_full_v14_release_capacity "$source_env" "$temporary"
  mv "$temporary" "$target"
  trap - EXIT
  printf '%s\n' "$target"
}

# produce one measured full-size ARM64 proof on an invocation-owned clone
historical_v14_18_21_rehearsal() (
  local source_env=$1
  local target_env=$2
  local database_name postgres_dir postgres_container_id postgres_identity architecture
  local candidate candidate_oid existing_candidate_oid migration_container migration_container_id migration_started=false
  local migration_image_reference migration_image_id compose_project
  local candidate_created=false samples monitor_completion monitor_pid
  local samples_json_temporary receipt_temporary source_history target_history
  local system_identifier database_oid database_bytes weather_rows weather_bytes
  local candidate_scan_rows candidate_scan_heap_bytes
  local largest_index_bytes max_wal_bytes source_database_file_count
  local free_bytes_before free_inodes_before start_wal_lsn peak_wal_bytes
  local before_data_sha256 started_at finished_at duration_milliseconds
  local preexisting_data_contract preexisting_data_before preexisting_data_after
  local required_historical_bytes required_historical_inodes details migration_status
  local sample_free_bytes sample_free_inodes sample_temp_bytes _sample
  local cleanup_failed=false
  candidate="weather_historical_v14_$(date -u +%Y%m%d%H%M%S)_$$"
  migration_container="${candidate}_migration"
  candidate_oid=
  migration_container_id=
  monitor_pid=
  capacity_monitor_next_sequence=
  database_name=$(env_value "$source_env" WEATHER_DATABASE_NAME)
  postgres_dir=$(env_value "$source_env" WEATHER_POSTGRES_DIR)
  samples=$(mktemp "$state_dir/.historical-v14-samples.XXXXXX")
  monitor_completion="$samples.complete"
  samples_json_temporary=$(mktemp "$state_dir/.historical-v14-samples-json.XXXXXX")
  receipt_temporary=$(mktemp "$state_dir/.historical-v14-receipt.XXXXXX")
  preexisting_data_contract=$(mktemp "$state_dir/.historical-v14-data-contract.XXXXXX")
  preexisting_data_before=$(mktemp "$state_dir/.historical-v14-data-before.XXXXXX")
  preexisting_data_after=$(mktemp "$state_dir/.historical-v14-data-after.XXXXXX")
  chmod 600 "$samples" "$samples_json_temporary" "$receipt_temporary" \
    "$preexisting_data_contract" "$preexisting_data_before" \
    "$preexisting_data_after"

  # remove every invocation-owned object without trusting a generated name alone
  # shellcheck disable=SC2317,SC2329
  cleanup_historical_rehearsal() {
    local status=$?
    local current_oid details
    trap - EXIT
    set +e
    if [[ -n "$monitor_pid" ]]; then
      kill "$monitor_pid" >/dev/null 2>&1
      wait "$monitor_pid" >/dev/null 2>&1
      monitor_pid=
    fi
    if [[ "$migration_started" == true ]]; then
      details=$(docker container inspect --format \
        '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
        "$migration_container_id" 2>/dev/null)
      if [[ "$details" == "$migration_container_id|/$migration_container|$compose_project|migration|$migration_image_reference|$migration_image_id" ]]; then
        docker container stop --time 10 "$migration_container_id" >/dev/null 2>&1
        details=$(docker container inspect --format \
          '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
          "$migration_container_id" 2>/dev/null)
        [[ "$details" == "$migration_container_id|/$migration_container|$compose_project|migration|$migration_image_reference|$migration_image_id" ]] &&
          docker container rm "$migration_container_id" >/dev/null 2>&1 || cleanup_failed=true
        docker container inspect "$migration_container_id" >/dev/null 2>&1 &&
          cleanup_failed=true
      else
        cleanup_failed=true
      fi
    fi
    if [[ "$candidate_created" == true ]]; then
      current_oid=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
        psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
          --tuples-only --no-align --command \
          "SELECT oid FROM pg_database WHERE datname = '$candidate'" 2>/dev/null)
      # recover the exact OID after a successful create whose first identity read failed
      if [[ ! "$candidate_oid" =~ ^[1-9][0-9]*$ &&
        "$current_oid" =~ ^[1-9][0-9]*$ ]]; then
        candidate_oid=$current_oid
      fi
      if [[ "$candidate_oid" =~ ^[1-9][0-9]*$ &&
        "$current_oid" == "$candidate_oid" ]]; then
        WEATHER_ENV_FILE=$source_env compose exec -T postgres \
          dropdb --username postgres "$candidate" >/dev/null 2>&1 || cleanup_failed=true
        current_oid=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
          psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
            --tuples-only --no-align --command \
            "SELECT oid FROM pg_database WHERE datname = '$candidate'" 2>/dev/null) ||
          cleanup_failed=true
        [[ -z "$current_oid" ]] || cleanup_failed=true
      else
        cleanup_failed=true
      fi
    fi
    # preserve bounded diagnostics after every owned process has stopped
    if [[ "$status" != 0 || "$cleanup_failed" == true ]]; then
      if [[ "$status" == 0 ]]; then
        status=1
      fi
      retain_release_failure_evidence historical-v14-rehearsal "$status" \
        "$samples" "$samples_json_temporary" "$receipt_temporary" \
        "$preexisting_data_contract" "$preexisting_data_before" \
        "$preexisting_data_after" || cleanup_failed=true
    fi
    rm -f -- "$samples" "$monitor_completion" "$samples_json_temporary" \
      "$receipt_temporary" "$preexisting_data_contract" \
      "$preexisting_data_before" "$preexisting_data_after"
    if [[ "$cleanup_failed" == true ]]; then
      exit 1
    fi
    exit "$status"
  }
  trap cleanup_historical_rehearsal EXIT
  trap 'exit 130' HUP INT TERM

  require_fixed_v14_source_identity "$target_env" "$source_env"
  require_normal_release_resource_gate
  require_literal_full_v14_release_capacity "$source_env" "$target_env"
  source_history=$(migration_history_sha256 "$source_env" "$database_name")
  [[ "$source_history" == e4a73765078fba8235bf94753c799c209d59f737dde6f38d1a1a458e3164a82e ]] ||
    die "historical rehearsal source is not the exact schema-18 ledger"
  postgres_container_id=$(WEATHER_ENV_FILE=$source_env compose ps -q postgres)
  [[ "$postgres_container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "historical rehearsal PostgreSQL identity is invalid"
  postgres_identity=$(docker container inspect --format \
    '{{.Id}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.Memory}}|{{.Image}}' \
    "$postgres_container_id")
  [[ "$postgres_identity" == "$postgres_container_id|postgres|1000000000|536870912|sha256:"* ]] ||
    die "historical rehearsal PostgreSQL quotas differ"
  architecture=$(docker image inspect --format '{{.Architecture}}' \
    "$(env_value "$source_env" POSTGRES_IMAGE)")
  [[ "$architecture" == arm64 && "$(uname -m)" == aarch64 ]] ||
    die "historical rehearsal requires the production ARM64 host"

  read -r system_identifier database_oid database_bytes weather_rows weather_bytes \
    largest_index_bytes max_wal_bytes < <(
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
        --tuples-only --no-align --field-separator=' ' --command \
        "SELECT (SELECT system_identifier FROM pg_control_system()), (SELECT oid FROM pg_database WHERE datname = current_database()), pg_database_size(current_database()), (SELECT count(*) FROM weather_records), pg_total_relation_size('weather_records'), COALESCE((SELECT max(pg_relation_size(indexrelid)) FROM pg_index JOIN pg_class ON pg_class.oid = pg_index.indrelid WHERE pg_class.relnamespace NOT IN ('pg_catalog'::regnamespace, 'information_schema'::regnamespace)), 0), pg_size_bytes(current_setting('max_wal_size'))")
  [[ -d "$postgres_dir/base/$database_oid" && ! -L "$postgres_dir/base/$database_oid" ]] ||
    die "historical rehearsal source file root is invalid"
  source_database_file_count=$(find -P "$postgres_dir/base/$database_oid" -type f -printf . | wc -c)
  read -r free_bytes_before _ free_inodes_before < <(
    stat --file-system --format='%a %S %d' "$postgres_dir" |
      awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }')
  required_historical_bytes=$((image_pull_required_free_bytes + database_bytes +
    largest_index_bytes + 2 * max_wal_bytes + 268435456))
  required_historical_inodes=$((image_pull_required_free_inodes +
    source_database_file_count + 4096))
  ((free_bytes_before >= required_historical_bytes &&
    free_inodes_before >= required_historical_inodes)) ||
    die "historical rehearsal lacks its full-copy capacity envelope"

  printf '0 %s %s %s 0\n' "$(date -u +%s%3N)" \
    "$free_bytes_before" "$free_inodes_before" >"$samples"
  (
    _next_sequence=1
    # publish one exact completion handshake only on an intentional stop
    # shellcheck disable=SC2317,SC2329
    complete_historical_monitor() {
      _completion_temporary="$monitor_completion.$BASHPID"
      printf '%s %s\n' "$BASHPID" "$_next_sequence" >"$_completion_temporary" || exit 1
      chmod 600 "$_completion_temporary" || exit 1
      mv -f -- "$_completion_temporary" "$monitor_completion" || exit 1
      exit 0
    }
    trap complete_historical_monitor TERM
    # retain the complete bounded free-space and PostgreSQL-temp series
    for ((_sample = 0; _sample < 4093; _sample += 1)); do
      read -r sample_free_bytes _ sample_free_inodes < <(
        stat --file-system --format='%a %S %d' "$postgres_dir" |
          awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }')
      sample_temp_bytes=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
        psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
          --tuples-only --no-align --command \
          "SELECT COALESCE(sum(size), 0) FROM pg_ls_tmpdir()")
      # append each sample with one writer and one complete printf
      printf '%s %s %s %s %s\n' "$_next_sequence" "$(date -u +%s%3N)" \
        "$sample_free_bytes" "$sample_free_inodes" "$sample_temp_bytes" || exit 1
      _next_sequence=$((_next_sequence + 1))
      sleep 1
    done
    exit 1
  ) >>"$samples" &
  monitor_pid=$!
  start_wal_lsn=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command 'SELECT pg_current_wal_lsn()')

  existing_candidate_oid=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command \
      "SELECT oid FROM pg_database WHERE datname = '$candidate'") ||
    die "historical rehearsal clone absence check failed"
  [[ -z "$existing_candidate_oid" ]] ||
    die "historical rehearsal clone name already exists"
  # capture a partial create before rejecting its failed command status
  if ! WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    createdb --username postgres --owner weather_owner --template template0 "$candidate"; then
    candidate_oid=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
        --tuples-only --no-align --command \
        "SELECT oid FROM pg_database WHERE datname = '$candidate'" 2>/dev/null || true)
    [[ "$candidate_oid" =~ ^[1-9][0-9]*$ ]] && candidate_created=true
    die "historical rehearsal clone creation failed"
  fi
  candidate_created=true
  candidate_oid=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command \
      "SELECT oid FROM pg_database WHERE datname = '$candidate'")
  [[ "$candidate_oid" =~ ^[1-9][0-9]*$ ]] ||
    die "historical rehearsal clone identity is invalid"
  WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    pg_dump --username weather_owner --dbname "$database_name" --format=custom --no-owner |
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      pg_restore --username postgres --dbname "$candidate" --no-owner \
        --role weather_owner --exit-on-error
  [[ "$(migration_history_sha256 "$source_env" "$candidate")" == "$source_history" ]] ||
    die "historical rehearsal clone ledger differs from its source"

  write_preexisting_data_contract "$source_env" "$candidate" \
    "$preexisting_data_contract"
  write_preexisting_data_snapshot "$source_env" "$candidate" \
    "$preexisting_data_contract" "$preexisting_data_before"
  before_data_sha256=$(cat "$preexisting_data_contract" "$preexisting_data_before" |
    sha256sum | awk '{print $1}')
  read -r candidate_scan_rows candidate_scan_heap_bytes < <(
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$candidate" \
        --tuples-only --no-align --field-separator=' ' --command \
        "SELECT (SELECT count(*) FROM weather_records) + (SELECT count(*) FROM forecast_anchor_records) + (SELECT count(*) FROM rain_adjustment_runs) + (SELECT count(*) FROM ecmwf_temperature_canary_runs), pg_relation_size('weather_records') + pg_relation_size('forecast_anchor_records') + pg_relation_size('rain_adjustment_runs') + pg_relation_size('ecmwf_temperature_canary_runs')")
  [[ "$candidate_scan_rows" =~ ^[1-9][0-9]*$ &&
    "$candidate_scan_heap_bytes" =~ ^[1-9][0-9]*$ ]] ||
    die "historical rehearsal scan workload envelope is empty or invalid"

  compose_project=${WEATHER_COMPOSE_PROJECT_NAME:-weather}
  migration_image_reference=$(env_value "$target_env" WEATHER_SERVER_IMAGE)
  migration_image_id=$(docker image inspect --format '{{.Id}}' "$migration_image_reference")
  ! docker container inspect "$migration_container" >/dev/null 2>&1 ||
    die "historical rehearsal migration name already exists"
  started_at=$(date -u +%s%3N)
  # capture and clean an exact task even when Compose reports launch failure
  if migration_container_id=$(WEATHER_ENV_FILE=$target_env compose run --detach \
    --name "$migration_container" --no-deps --env WEATHER_DATABASE_NAME="$candidate" \
    --env WEATHER_DATABASE_STATEMENT_TIMEOUT_MS=300000 \
    --env WEATHER_DATABASE_LOCK_TIMEOUT_MS=10000 migration \
    node deploy/scripts/migrate.mjs --atomic-maintenance-v14); then
    :
  else
    migration_container_id=$(docker container inspect --format '{{.Id}}' \
      "$migration_container" 2>/dev/null) ||
      die "historical rehearsal migration failed before creation"
    migration_started=true
    die "historical rehearsal migration launch failed"
  fi
  # bind the successful create to its exact ID, labels and image before waiting
  if [[ ! "$migration_container_id" =~ ^[a-f0-9]{64}$ ]]; then
    migration_container_id=$(docker container inspect --format '{{.Id}}' \
      "$migration_container" 2>/dev/null) ||
      die "historical rehearsal migration container identity is invalid"
  fi
  migration_started=true
  details=$(docker container inspect --format \
    '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
    "$migration_container_id")
  [[ "$details" == "$migration_container_id|/$migration_container|$compose_project|migration|$migration_image_reference|$migration_image_id" ]] ||
    die "historical rehearsal migration container identity differs"
  migration_status=$(docker container wait "$migration_container_id")
  if [[ "$migration_status" != 0 ]]; then
    docker container logs "$migration_container_id" >&2 || true
    die "historical rehearsal atomic migration failed"
  fi
  finished_at=$(date -u +%s%3N)
  duration_milliseconds=$((finished_at - started_at))
  ((duration_milliseconds <= 150000)) ||
    die "historical rehearsal exceeds the reviewed two-times duration headroom"
  target_history=$(migration_history_sha256 "$source_env" "$candidate")
  [[ "$target_history" == adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93 ]] ||
    die "historical rehearsal target is not the exact schema-21 ledger"
  write_preexisting_data_snapshot "$source_env" "$candidate" \
    "$preexisting_data_contract" "$preexisting_data_after"
  verify_preexisting_data_snapshots "$preexisting_data_contract" \
    "$preexisting_data_before" "$preexisting_data_after" ||
    die "historical rehearsal changed preexisting application data"
  peak_wal_bytes=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
      --tuples-only --no-align --command \
      "SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), '$start_wal_lsn'::pg_lsn)::bigint")

  details=$(docker container inspect --format \
    '{{.Id}}|{{.Name}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}|{{.Config.Image}}|{{.Image}}' \
    "$migration_container_id")
  [[ "$details" == "$migration_container_id|/$migration_container|$compose_project|migration|$migration_image_reference|$migration_image_id" ]] ||
    die "historical rehearsal migration cleanup identity differs"
  docker container rm "$migration_container_id" >/dev/null
  ! docker container inspect "$migration_container_id" >/dev/null 2>&1 ||
    die "historical rehearsal migration cleanup was not confirmed"
  migration_started=false
  [[ "$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command \
      "SELECT oid FROM pg_database WHERE datname = '$candidate'")" == "$candidate_oid" ]] ||
    die "historical rehearsal clone cleanup identity differs"
  WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    dropdb --username postgres "$candidate"
  [[ -z "$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command \
      "SELECT oid FROM pg_database WHERE datname = '$candidate'")" ]] ||
    die "historical rehearsal clone cleanup was not confirmed"
  candidate_created=false
  # require the monitor to be live and to close its complete sequence explicitly
  stop_owned_capacity_monitor "$monitor_pid" "$monitor_completion" "$samples" ||
    die "historical rehearsal capacity monitor died or ended incompletely"
  monitor_pid=
  read -r sample_free_bytes _ sample_free_inodes < <(
    stat --file-system --format='%a %S %d' "$postgres_dir" |
      awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }')
  sample_temp_bytes=$(WEATHER_ENV_FILE=$source_env compose exec -T postgres \
    psql --no-psqlrc --set=ON_ERROR_STOP=1 --username postgres --dbname postgres \
      --tuples-only --no-align --command "SELECT COALESCE(sum(size), 0) FROM pg_ls_tmpdir()")
  printf '%s %s %s %s %s\n' "$capacity_monitor_next_sequence" "$(date -u +%s%3N)" \
    "$sample_free_bytes" "$sample_free_inodes" "$sample_temp_bytes" >>"$samples"

  node --input-type=module - "$samples" "$samples_json_temporary" "$receipt_temporary" \
    "$(sha256sum "$source_env" | awk '{print $1}')" \
    "$(sha256sum "$target_env" | awk '{print $1}')" "$system_identifier" \
    "$database_oid" "$database_bytes" "$weather_rows" "$weather_bytes" \
    "$before_data_sha256" "$candidate_scan_rows" "$candidate_scan_heap_bytes" \
    "$largest_index_bytes" "$max_wal_bytes" "$source_database_file_count" \
    "$free_bytes_before" "$free_inodes_before" "$required_historical_bytes" \
    "$required_historical_inodes" "$duration_milliseconds" "$peak_wal_bytes" \
    "$(env_value "$source_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$source_env" WEATHER_WEB_IMAGE)" \
    "$(env_value "$target_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$target_env" WEATHER_WEB_IMAGE)" <<'NODE'
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const [samplesPath, samplesOutput, output, sourceEnvSha256, targetEnvSha256, systemIdentifier,
  databaseOid, databaseBytes, weatherRecordCount, weatherRecordBytes,
  candidateAppDataSha256, candidateScanRowCount, candidateScanHeapBytes,
  largestSourceIndexBytes, maxWalBytes, sourceDatabaseFileCount, freeBytesBefore,
  freeInodesBefore, requiredFreeBytes, requiredFreeInodes, durationMilliseconds,
  peakWalBytes, sourceServerImage, sourceWebImage, targetServerImage,
  targetWebImage] = process.argv.slice(2);
const lines = readFileSync(samplesPath, "utf8").trimEnd().split("\n");
if (lines.length < 2 || lines.length > 4_096) throw new Error("historical sample count is invalid");
const samples = lines.map((line) => line.split(" ").map(Number));
if (samples.some((sample) => sample.length !== 5 || sample.some((value) =>
  !Number.isSafeInteger(value) || value < 0))) throw new Error("historical sample is invalid");
for (let index = 0; index < samples.length; index += 1) {
  if (samples[index][0] !== index ||
    (index > 0 && samples[index][1] < samples[index - 1][1])) {
    throw new Error("historical sample sequence is incomplete");
  }
}
const retainedSamples = samples.map(([sequence, capturedAtMilliseconds, freeBytes,
  freeInodes, tempBytes]) => ({ capturedAtMilliseconds, freeBytes, freeInodes,
  sequence, tempBytes }));
const minimumFreeBytesObserved = Math.min(...samples.map((sample) => sample[2]));
const minimumFreeInodesObserved = Math.min(...samples.map((sample) => sample[3]));
const peakTempBytes = Math.max(...samples.map((sample) => sample[4]));
const peakGrowthBytes = Number(freeBytesBefore) - minimumFreeBytesObserved;
const peakGrowthInodes = Number(freeInodesBefore) - minimumFreeInodesObserved;
if (peakGrowthBytes < 0 || peakGrowthInodes < 0 ||
  minimumFreeBytesObserved < 2_034_155_520 || minimumFreeInodesObserved < 32_768 ||
  peakTempBytes > 268_435_456 || Number(peakWalBytes) > 2 * Number(maxWalBytes)) {
  throw new Error("historical rehearsal exceeded its measured envelope");
}
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
const samplesBody = {
  contractVersion: "historical-v14-capacity-samples/v1",
  samples: retainedSamples,
};
const samplesReceipt = { ...samplesBody,
  receiptSha256: createHash("sha256").update(canonical(samplesBody)).digest("hex") };
const samplesBytes = Buffer.from(canonical(samplesReceipt));
writeFileSync(samplesOutput, samplesBytes, { mode: 0o600 });
const body = {
  appDataEqual: true,
  architecture: "aarch64",
  candidateAppDataSha256,
  candidateScanHeapBytes: Number(candidateScanHeapBytes),
  candidateScanRowCount: Number(candidateScanRowCount),
  capacitySampleCount: retainedSamples.length,
  capacitySamplesReceiptSha256: samplesReceipt.receiptSha256,
  capacitySamplesSha256: createHash("sha256").update(samplesBytes).digest("hex"),
  capturedAt: new Date().toISOString(),
  cleanupConfirmed: true,
  contractVersion: "historical-v14-18-21-rehearsal/v1",
  databaseBytes: Number(databaseBytes),
  databaseOid: Number(databaseOid),
  durationMilliseconds: Number(durationMilliseconds),
  freeBytesBefore: Number(freeBytesBefore),
  freeInodesBefore: Number(freeInodesBefore),
  largestSourceIndexBytes: Number(largestSourceIndexBytes),
  lockTimeoutMilliseconds: 10000,
  maxWalBytes: Number(maxWalBytes),
  memoryLimitBytes: 512 * 1024 * 1024,
  minimumFreeBytesObserved,
  minimumFreeInodesObserved,
  peakGrowthBytes,
  peakGrowthInodes,
  peakTempBytes,
  peakWalBytes: Number(peakWalBytes),
  requiredFreeBytes: Number(requiredFreeBytes),
  requiredFreeInodes: Number(requiredFreeInodes),
  sourceDatabaseFileCount: Number(sourceDatabaseFileCount),
  sourceEnvSha256,
  sourceLedgerSha256: "e4a73765078fba8235bf94753c799c209d59f737dde6f38d1a1a458e3164a82e",
  sourceRelease: "2026.10.09-2",
  sourceServerImage,
  sourceWebImage,
  statementTimeoutMilliseconds: 300000,
  systemIdentifier,
  targetEnvSha256,
  targetLedgerSha256: "adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93",
  targetRelease: "2026.10.09-3",
  targetServerImage,
  targetWebImage,
  weatherRecordBytes: Number(weatherRecordBytes),
  weatherRecordCount: Number(weatherRecordCount),
};
const receipt = { ...body,
  receiptSha256: createHash("sha256").update(canonical(body)).digest("hex") };
writeFileSync(output, canonical(receipt), { mode: 0o600 });
NODE
  require_normal_release_resource_gate
  require_literal_full_v14_release_capacity "$source_env" "$target_env"
  require_historical_v14_rehearsal_receipt \
    "$source_env" "$target_env" "$receipt_temporary" "$samples_json_temporary"
  mv -f -- "$samples_json_temporary" "$historical_v14_rehearsal_samples"
  samples_json_temporary=
  chmod 600 "$historical_v14_rehearsal_samples"
  sync -f "$historical_v14_rehearsal_samples" "$state_dir"
  mv -f -- "$receipt_temporary" "$historical_v14_rehearsal_receipt"
  receipt_temporary=
  chmod 600 "$historical_v14_rehearsal_receipt"
  sync -f "$historical_v14_rehearsal_receipt" "$state_dir"
  require_historical_v14_rehearsal_receipt "$source_env" "$target_env"
  rm -f -- "$samples" "$monitor_completion"
  trap - EXIT
  printf 'Historical v14 full-size rehearsal passed and was sealed.\n'
)

# require one measured full-size ARM64 proof for the historical 18-to-21 handoff
require_historical_v14_rehearsal_receipt() {
  local source_env=$1
  local target_env=$2
  local receipt_path=${3:-$historical_v14_rehearsal_receipt}
  local samples_path=${4:-$historical_v14_rehearsal_samples}
  local database_name receipt_mode receipt_size source_env_sha256 target_env_sha256
  local source_history system_identifier database_oid database_bytes weather_rows weather_bytes
  local live_scan_rows live_scan_heap_bytes largest_index_bytes max_wal_bytes
  local source_database_file_count postgres_dir free_bytes free_inodes
  database_name=$(env_value "$source_env" WEATHER_DATABASE_NAME)
  require_canonical_descendant "$receipt_path" "$state_dir" \
    "historical v14 rehearsal receipt"
  require_file "$receipt_path"
  [[ ! -L "$receipt_path" ]] ||
    die "historical v14 rehearsal receipt must not be linked"
  receipt_mode=$(stat -c '%u:%a' "$receipt_path")
  receipt_size=$(stat -c '%s' "$receipt_path")
  [[ "$receipt_mode" == "$EUID:600" && "$receipt_size" -ge 2 &&
    "$receipt_size" -le 16384 ]] ||
    die "historical v14 rehearsal receipt metadata is invalid"
  require_canonical_descendant "$samples_path" "$state_dir" \
    "historical v14 rehearsal samples"
  require_file "$samples_path"
  [[ ! -L "$samples_path" && "$(stat -c '%u:%a' "$samples_path")" == "$EUID:600" &&
    "$(stat -c '%s' "$samples_path")" -ge 2 &&
    "$(stat -c '%s' "$samples_path")" -le 1048576 ]] ||
    die "historical v14 rehearsal sample metadata is invalid"
  source_env_sha256=$(sha256sum "$source_env" | awk '{print $1}')
  target_env_sha256=$(sha256sum "$target_env" | awk '{print $1}')
  source_history=$(migration_history_sha256 "$source_env" "$database_name")
  read -r system_identifier database_oid database_bytes weather_rows weather_bytes \
    live_scan_rows live_scan_heap_bytes largest_index_bytes max_wal_bytes < <(
    WEATHER_ENV_FILE=$source_env compose exec -T postgres \
      psql --set=ON_ERROR_STOP=1 --username postgres --dbname "$database_name" \
        --tuples-only --no-align --field-separator=' ' --command \
        "SELECT (SELECT system_identifier FROM pg_control_system()), (SELECT oid FROM pg_database WHERE datname = current_database()), pg_database_size(current_database()), (SELECT count(*) FROM weather_records), pg_total_relation_size('weather_records'), (SELECT count(*) FROM weather_records) + (SELECT count(*) FROM forecast_anchor_records) + (SELECT count(*) FROM rain_adjustment_runs) + (SELECT count(*) FROM ecmwf_temperature_canary_runs), pg_relation_size('weather_records') + pg_relation_size('forecast_anchor_records') + pg_relation_size('rain_adjustment_runs') + pg_relation_size('ecmwf_temperature_canary_runs'), COALESCE((SELECT max(pg_relation_size(indexrelid)) FROM pg_index JOIN pg_class ON pg_class.oid = pg_index.indrelid WHERE pg_class.relnamespace NOT IN ('pg_catalog'::regnamespace, 'information_schema'::regnamespace)), 0), pg_size_bytes(current_setting('max_wal_size'))")
  postgres_dir=$(env_value "$source_env" WEATHER_POSTGRES_DIR)
  [[ -d "$postgres_dir/base/$database_oid" && ! -L "$postgres_dir/base/$database_oid" ]] ||
    die "historical v14 source file root is invalid"
  source_database_file_count=$(find -P "$postgres_dir/base/$database_oid" -type f -printf . | wc -c)
  read -r free_bytes _ free_inodes < <(
    stat --file-system --format='%a %S %d' "$postgres_dir" |
      awk '{ printf "%.0f %s %s\n", $1 * $2, $2, $3 }')
  node --input-type=module - "$receipt_path" "$samples_path" \
    "$source_env_sha256" "$target_env_sha256" "$system_identifier" "$database_oid" \
    "$database_bytes" "$weather_rows" "$weather_bytes" "$live_scan_rows" \
    "$live_scan_heap_bytes" "$largest_index_bytes" "$max_wal_bytes" \
    "$source_database_file_count" "$free_bytes" "$free_inodes" \
    "$(env_value "$source_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$source_env" WEATHER_WEB_IMAGE)" \
    "$(env_value "$target_env" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$target_env" WEATHER_WEB_IMAGE)" "$source_history" <<'NODE'
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const [path, samplesPath, sourceEnvSha256, targetEnvSha256, systemIdentifier, databaseOid,
  databaseBytes, weatherRows, weatherBytes, liveScanRows, liveScanHeapBytes,
  largestSourceIndexBytes, maxWalBytes, sourceDatabaseFileCount, freeBytes, freeInodes,
  sourceServerImage, sourceWebImage,
  targetServerImage, targetWebImage, sourceHistory] = process.argv.slice(2);
const receipt = JSON.parse(readFileSync(path, "utf8"));
const samplesBytes = readFileSync(samplesPath);
const retained = JSON.parse(samplesBytes);
const keys = ["appDataEqual", "architecture", "candidateAppDataSha256",
  "candidateScanHeapBytes", "candidateScanRowCount", "capacitySampleCount",
  "capacitySamplesReceiptSha256", "capacitySamplesSha256", "capturedAt", "cleanupConfirmed",
  "contractVersion", "databaseBytes", "databaseOid", "durationMilliseconds",
  "freeBytesBefore", "freeInodesBefore", "largestSourceIndexBytes",
  "lockTimeoutMilliseconds", "maxWalBytes", "memoryLimitBytes",
  "minimumFreeBytesObserved", "minimumFreeInodesObserved", "peakGrowthBytes",
  "peakGrowthInodes", "peakTempBytes", "peakWalBytes", "requiredFreeBytes",
  "requiredFreeInodes",
  "receiptSha256", "sourceEnvSha256", "sourceLedgerSha256", "sourceRelease",
  "sourceDatabaseFileCount", "sourceServerImage", "sourceWebImage",
  "statementTimeoutMilliseconds",
  "systemIdentifier", "targetEnvSha256", "targetLedgerSha256", "targetRelease",
  "targetServerImage", "targetWebImage", "weatherRecordBytes", "weatherRecordCount"];
const exactKeys = Object.keys(receipt).sort().join("\n") === keys.sort().join("\n");
const canonical = (value) => Array.isArray(value)
  ? `[${value.map(canonical).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
    : JSON.stringify(value);
const body = { ...receipt };
delete body.receiptSha256;
const digest = createHash("sha256").update(canonical(body)).digest("hex");
const capturedAt = Date.parse(receipt.capturedAt);
const age = Date.now() - capturedAt;
const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const receiptRequiredFreeBytes = 2_030_043_136 + 4_112_384 + receipt.databaseBytes +
  receipt.largestSourceIndexBytes + 2 * receipt.maxWalBytes + 268_435_456;
const receiptRequiredFreeInodes = 32_768 + receipt.sourceDatabaseFileCount + 4_096;
const liveRequiredFreeBytes = 2_030_043_136 + 4_112_384 + Number(databaseBytes) +
  Number(largestSourceIndexBytes) + 2 * Number(maxWalBytes) + 268_435_456;
const liveRequiredFreeInodes = 32_768 + Number(sourceDatabaseFileCount) + 4_096;
const retainedBody = { ...retained };
delete retainedBody.receiptSha256;
const retainedDigest = createHash("sha256").update(canonical(retainedBody)).digest("hex");
const retainedSamples = retained.samples;
let retainedSeriesValid = retained.contractVersion === "historical-v14-capacity-samples/v1" &&
  Object.keys(retained).sort().join("\n") === ["contractVersion", "receiptSha256", "samples"].sort().join("\n") &&
  retained.receiptSha256 === retainedDigest && Array.isArray(retainedSamples) &&
  retainedSamples.length >= 2 && retainedSamples.length <= 4_096;
// reject a truncated, reordered or noncanonical retained sample series
if (retainedSeriesValid) {
  for (let index = 0; index < retainedSamples.length; index += 1) {
    const sample = retainedSamples[index];
    if (Object.keys(sample).sort().join("\n") !==
      ["capturedAtMilliseconds", "freeBytes", "freeInodes", "sequence", "tempBytes"].sort().join("\n") ||
      sample.sequence !== index || !integer(sample.capturedAtMilliseconds) ||
      !integer(sample.freeBytes) || !integer(sample.freeInodes) || !integer(sample.tempBytes) ||
      (index > 0 && sample.capturedAtMilliseconds < retainedSamples[index - 1].capturedAtMilliseconds)) {
      retainedSeriesValid = false;
      break;
    }
  }
}
const retainedMinimumBytes = retainedSeriesValid
  ? Math.min(...retainedSamples.map((sample) => sample.freeBytes)) : -1;
const retainedMinimumInodes = retainedSeriesValid
  ? Math.min(...retainedSamples.map((sample) => sample.freeInodes)) : -1;
const retainedPeakTemp = retainedSeriesValid
  ? Math.max(...retainedSamples.map((sample) => sample.tempBytes)) : -1;
const liveRows = BigInt(liveScanRows);
const liveHeapBytes = BigInt(liveScanHeapBytes);
const candidateRows = BigInt(receipt.candidateScanRowCount ?? 0);
const candidateHeapBytes = BigInt(receipt.candidateScanHeapBytes ?? 0);
const duration = BigInt(receipt.durationMilliseconds ?? 0);
if (!exactKeys || receipt.contractVersion !== "historical-v14-18-21-rehearsal/v1" ||
  receipt.sourceRelease !== "2026.10.09-2" || receipt.targetRelease !== "2026.10.09-3" ||
  receipt.sourceEnvSha256 !== sourceEnvSha256 || receipt.targetEnvSha256 !== targetEnvSha256 ||
  receipt.systemIdentifier !== systemIdentifier || receipt.databaseOid !== Number(databaseOid) ||
  !integer(receipt.databaseBytes) || !integer(receipt.weatherRecordCount) ||
  !integer(receipt.weatherRecordBytes) || !integer(receipt.largestSourceIndexBytes) ||
  !integer(receipt.maxWalBytes) || !integer(receipt.sourceDatabaseFileCount) ||
  receipt.requiredFreeBytes !== receiptRequiredFreeBytes ||
  receipt.requiredFreeInodes !== receiptRequiredFreeInodes ||
  !/^[a-f0-9]{64}$/u.test(receipt.candidateAppDataSha256) ||
  !integer(receipt.candidateScanRowCount) || receipt.candidateScanRowCount < 1 ||
  !integer(receipt.candidateScanHeapBytes) || receipt.candidateScanHeapBytes < 1 ||
  !retainedSeriesValid || receipt.capacitySampleCount !== retainedSamples.length ||
  receipt.capacitySamplesReceiptSha256 !== retained.receiptSha256 ||
  receipt.capacitySamplesSha256 !== createHash("sha256").update(samplesBytes).digest("hex") ||
  receipt.minimumFreeBytesObserved !== retainedMinimumBytes ||
  receipt.minimumFreeInodesObserved !== retainedMinimumInodes ||
  receipt.peakTempBytes !== retainedPeakTemp ||
  receipt.freeBytesBefore !== retainedSamples[0]?.freeBytes ||
  receipt.freeInodesBefore !== retainedSamples[0]?.freeInodes ||
  receipt.peakGrowthBytes !== receipt.freeBytesBefore - retainedMinimumBytes ||
  receipt.peakGrowthInodes !== receipt.freeInodesBefore - retainedMinimumInodes ||
  receipt.sourceServerImage !== sourceServerImage || receipt.sourceWebImage !== sourceWebImage ||
  receipt.targetServerImage !== targetServerImage || receipt.targetWebImage !== targetWebImage ||
  receipt.sourceLedgerSha256 !== sourceHistory ||
  sourceHistory !== "e4a73765078fba8235bf94753c799c209d59f737dde6f38d1a1a458e3164a82e" ||
  receipt.targetLedgerSha256 !== "adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93" ||
  receipt.architecture !== "aarch64" || receipt.memoryLimitBytes !== 512 * 1024 * 1024 ||
  receipt.statementTimeoutMilliseconds !== 300000 || receipt.lockTimeoutMilliseconds !== 10000 ||
  !integer(receipt.durationMilliseconds) || receipt.durationMilliseconds < 1 ||
  receipt.durationMilliseconds > 150000 || candidateRows < 1n || candidateHeapBytes < 1n ||
  duration * liveRows > 150000n * candidateRows ||
  duration * liveHeapBytes > 150000n * candidateHeapBytes ||
  !integer(receipt.peakGrowthBytes) || !integer(receipt.peakGrowthInodes) ||
  !integer(receipt.peakTempBytes) || receipt.peakTempBytes > 268_435_456 ||
  !integer(receipt.peakWalBytes) || receipt.peakWalBytes > 2 * receipt.maxWalBytes ||
  !integer(receipt.freeBytesBefore) || receipt.freeBytesBefore < receiptRequiredFreeBytes ||
  !integer(receipt.freeInodesBefore) || receipt.freeInodesBefore < receiptRequiredFreeInodes ||
  Number(freeBytes) < liveRequiredFreeBytes || Number(freeInodes) < liveRequiredFreeInodes ||
  !integer(receipt.minimumFreeBytesObserved) || !integer(receipt.minimumFreeInodesObserved) ||
  receipt.minimumFreeBytesObserved < 2_034_155_520 ||
  receipt.minimumFreeInodesObserved < 32_768 ||
  receipt.appDataEqual !== true || receipt.cleanupConfirmed !== true ||
  !Number.isFinite(capturedAt) || age < -30_000 || age > 3_600_000 ||
  receipt.receiptSha256 !== digest) {
  throw new Error("historical v14 rehearsal receipt differs from the live handoff");
}
NODE
}

# apply one source-preserving direct release without deployment-time backup
inert_v14_release() (
  local release=$1
  local source_env=$2
  local target current v14_current_env inert_v14_authorization_path v14_temporary_authorization
  local actual_history expected_history v14_source_history restored_history schema_after_restore key
  local v14_starting_schema
  local git_proof target_commit catalog_sha256 settings_sha256 settings_inode
  local api_writer_id worker_writer_id
  local previous_before=
  local previous_existed=false
  local active_mutation_started=false
  local completed=false
  # shellcheck disable=SC2030
  current=$(read_optional_release_state "$state_dir/current-release")

  # restore the exact retained source after every active-database mutation failure
  # shellcheck disable=SC2317,SC2329
  restore_failed_inert_v14() {
    local status=$?
    local recovery_status=0
    trap - EXIT
    set +e

    # remove only an unpublished compatibility receipt
    if [[ -n "${v14_temporary_authorization:-}" ]]; then
      rm -f -- "$v14_temporary_authorization"
    fi

    # compensate only after the live database or service transaction began
    if [[ "$completed" != true && "$active_mutation_started" == true ]]; then
      printf 'Direct deployment failed; restoring Weather release %s...\n' \
        "$current" >&2
      # observe the real schema before injecting source application authorization
      expected_history=$(env_value \
        "$inert_v14_authorization_path" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256) ||
        recovery_status=1
      start_postgres "$v14_current_env" || recovery_status=1
      restored_history=$(migration_history_sha256 \
        "$v14_current_env" "$(env_value "$v14_current_env" WEATHER_DATABASE_NAME)") ||
        recovery_status=1
      schema_after_restore=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
        inert-v14-restoration-schema "$restored_history" "$v14_source_history" \
        "$current" "$v14_starting_schema" "$expected_history" "$release") || recovery_status=1
      if [[ "$recovery_status" == 0 ]] &&
        restore_v14_compatibility_bridge_images \
          "$v14_current_env" "$current" "$schema_after_restore"; then
        # remove only an exactly published actionless authority after source health is proven
        if [[ -n "${catalog_sha256:-}" && -n "${settings_sha256:-}" ]]; then
          node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" discard-failed-inert-v14-current \
            "$release" "$target_commit" "$(env_value "$target" WEATHER_SERVER_IMAGE)" \
            "$(env_value "$target" WEATHER_WEB_IMAGE)" "$catalog_sha256" "$settings_sha256" >/dev/null ||
            recovery_status=1
        fi
        # publish a schema marker only for an exact recognized ledger
        if [[ -n "$schema_after_restore" ]]; then
          write_private_state "$state_dir/schema-release" "$schema_after_restore" ||
            recovery_status=1
        fi
        write_active_symlink "$current" || recovery_status=1
        write_private_state "$state_dir/current-release" "$current" || recovery_status=1

        # restore the pre-transaction rollback marker after a partial commit
        if [[ "$previous_existed" == true ]]; then
          write_private_state "$state_dir/previous-release" "$previous_before" ||
            recovery_status=1
        else
          rm -f -- "$state_dir/previous-release" || recovery_status=1
        fi

        # report compensation capacity only after source state is durable
        require_literal_full_v14_release_capacity "$source_env" "$target" ||
          recovery_status=1
      else
        recovery_status=1
      fi

      # never report an original or compensation failure as a successful release
      if ((recovery_status != 0)); then
        printf 'Direct deployment and exact source restoration both failed.\n' >&2
        exit 1
      fi
    fi
    exit "$status"
  }
  trap restore_failed_inert_v14 EXIT

  # require one exact retained predecessor before rendering or pulling images
  [[ "$current" == "$v14_compatibility_bridge_release" ]] ||
    die "full v14 requires the exact committed compatibility bridge"
  require_v14_full_source_authority "$source_env"
  v14_starting_schema=$(read_release_state "$state_dir/schema-release")

  # freeze the rollback marker before record_release_success can replace it
  if [[ -e "$state_dir/previous-release" || -L "$state_dir/previous-release" ]]; then
    previous_before=$(read_release_state "$state_dir/previous-release")
    previous_existed=true
  fi
  v14_current_env=$(release_env "$current")
  validate_release_env "$v14_current_env" "$current"
  validate_release_env "$source_env" "$current"
  require_control_plane_compatibility "$v14_current_env"
  require_control_plane_compatibility "$source_env"

  # prevent an explicit source file from changing any retained release setting
  for key in WEATHER_RELEASE WEATHER_SERVER_IMAGE WEATHER_WEB_IMAGE POSTGRES_IMAGE \
    CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH \
    WEATHER_CONTROL_PLANE_SHA256 WEATHER_CONTROL_PLANE_VERSION; do
    [[ "$(env_value "$source_env" "$key")" == "$(env_value "$v14_current_env" "$key")" ]] ||
      die "direct deployment source differs from the retained release: $key"
  done

  git_proof=$(node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" verify-full-v14-release "$release") ||
    die "full v14 exact-commit CI proof failed"
  target_commit=$(node --input-type=module -e '
const proof = JSON.parse(process.argv[1]);
// require the independently resolved literal commit
if (!/^[a-f0-9]{40}$/.test(proof.targetCommit)) throw new Error("invalid target commit");
process.stdout.write(proof.targetCommit);
' "$git_proof")
  target=$(prepare_inert_v14_release "$release" "$source_env")
  require_fixed_v14_source_identity "$target" "$source_env"
  require_inert_v14_empty_catalog
  require_control_plane_compatibility "$target"
  require_deployment_secrets
  inert_v14_authorization_path=$(migration_authorization "$release")
  # a compensated full-schema retry must reuse its exact durable authorization
  if [[ "$v14_starting_schema" == "$full_v14_release" ]]; then
    validate_migration_authorization \
      "$inert_v14_authorization_path" "$current" "$release"
  else
    v14_temporary_authorization=$(mktemp \
      "$releases_dir/.${release}.XXXXXX.migration-authorization.partial")
    verify_fixed_v14_source_compatibility \
      "$target" "$source_env" "$v14_temporary_authorization"

    # reuse only byte-identical previously proven compatibility authorization
    if [[ -e "$inert_v14_authorization_path" || -L "$inert_v14_authorization_path" ]]; then
      validate_migration_authorization \
        "$inert_v14_authorization_path" "$current" "$release"
      cmp -s "$v14_temporary_authorization" "$inert_v14_authorization_path" ||
        die "existing fixed v14 migration authorization differs"
      rm -f -- "$v14_temporary_authorization"
      v14_temporary_authorization=
    else
      publish_migration_authorization \
        "$v14_temporary_authorization" "$inert_v14_authorization_path"
      v14_temporary_authorization=
    fi
  fi

  # bind compensation to the exact complete source ledger observed before mutation
  v14_source_history=$(migration_history_sha256 \
    "$v14_current_env" "$(env_value "$v14_current_env" WEATHER_DATABASE_NAME)")
  [[ "$v14_source_history" =~ ^[a-f0-9]{64}$ ]] ||
    die "fixed v14 source migration ledger is invalid"

  # require the measured full-size atomic proof before the last reversible boundary
  require_historical_v14_rehearsal_receipt "$source_env" "$target" ||
    die "full-size historical v14 rehearsal receipt is absent or stale"

  # remeasure fresh resources and literal images at the last reversible boundary
  require_normal_release_resource_gate ||
    die "fixed v14 resource gate failed before activation"
  require_literal_full_v14_release_capacity "$source_env" "$target" ||
    die "full v14 literal capacity failed before activation"

  # bind both supported writers before crossing the reversible stop boundary
  api_writer_id=$(capture_supported_writer_container "$v14_current_env" api)
  worker_writer_id=$(capture_supported_writer_container "$v14_current_env" worker)

  printf 'Applying release %s directly...\n' "$release"
  active_mutation_started=true
  stop_supported_writer_container "$v14_current_env" api "$api_writer_id" ||
    die "fixed v14 API writer quiescence failed"
  stop_supported_writer_container "$v14_current_env" worker "$worker_writer_id" ||
    die "fixed v14 worker quiescence failed"
  require_supported_writers_quiesced "$v14_current_env"
  # remeasure the prospective workload only after every supported writer is stopped
  require_historical_v14_rehearsal_receipt "$source_env" "$target" ||
    die "full-size historical v14 envelope changed after writer quiescence"
  start_postgres "$target" || die "fixed v14 PostgreSQL restart failed"
  require_supported_writers_quiesced "$target"
  require_historical_v14_rehearsal_receipt "$source_env" "$target" ||
    die "full-size historical v14 envelope changed after PostgreSQL restart"
  # keep the exact maintenance tail atomic for source-safe failure recovery
  WEATHER_ENV_FILE=$target compose run --rm \
    --env WEATHER_DATABASE_STATEMENT_TIMEOUT_MS=300000 \
    --env WEATHER_DATABASE_LOCK_TIMEOUT_MS=10000 migration \
    node deploy/scripts/migrate.mjs --atomic-maintenance-v14 ||
    die "fixed v14 active migration failed"
  actual_history=$(migration_history_sha256 \
    "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)")
  expected_history=$(env_value \
    "$inert_v14_authorization_path" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)
  [[ "$actual_history" == "$expected_history" ]] ||
    die "active migration ledger differs from fixed v14 compatibility proof"
  write_private_state "$state_dir/schema-release" "$release" ||
    die "fixed v14 schema marker publication failed"
  apply_runtime_database_acl "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)" ||
    die "fixed v14 runtime ACL application failed"
  verify_runtime_database_acl "$target" "$(env_value "$target" WEATHER_DATABASE_NAME)" ||
    die "fixed v14 runtime ACL verification failed"
  # freeze the actual database capture epoch before any new producer can stage
  "$deploy_dir/scripts/adjustment-evaluation-export.sh" --revision-capture-epoch-init-v1 "$target" ||
    die "fixed v14 future-only epoch witness publication failed"
  # provision the isolated twelve-gauge target sources before any producer starts
  initialize_adjustment_rain_fixed_gauge_target_sources_v1 "$target" ||
    die "fixed v14 rain target source initialization failed"
  # freeze validated operator intent before the new writable web process starts
  read -r settings_inode settings_sha256 < <(
    node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" inert-v14-settings-snapshot
  )
  [[ "$settings_inode" =~ ^[0-9]+:[0-9]+$ && "$settings_sha256" =~ ^[a-f0-9]{64}$ ]] ||
    die "inactive v14 settings snapshot is unavailable"
  start_exact_release "$target" || die "fixed v14 target release health failed"
  require_literal_full_v14_release_capacity "$source_env" "$target" ||
    die "full v14 idle literal capacity failed"
  catalog_sha256=$(sha256sum "$state_dir/adjustment-candidate-catalog.json" | awk '{print $1}')
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
    verify-inert-v14-settings-snapshot "$settings_inode" "$settings_sha256" ||
    die "inactive v14 operator settings changed before publication"
  node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" bootstrap-inert-v14-current \
    "$release" "$target_commit" "$(env_value "$target" WEATHER_SERVER_IMAGE)" \
    "$(env_value "$target" WEATHER_WEB_IMAGE)" "$catalog_sha256" "$settings_sha256" >/dev/null ||
    die "inactive v14 root source authority publication failed"
  # commit current only after every root source authority is durable
  record_release_success "$release" "$current" ||
    die "fixed v14 release state publication failed"
  completed=true
  trap - EXIT
  printf 'Release %s is active after direct deployment.\n' "$release"
)

# activate one forward release
start_release() (
  local target=$1
  local activation_env current current_env authorization_path schema_release stage_receipt
  local pre_capacity_input pre_capacity_receipt post_capacity_input post_capacity_receipt
  local backup_output backup_archive backup_bytes backup_ceiling
  local backup_checksum
  local migration_container_name migration_container_id migration_status actual_history
  local target_history=1db25d8a515e85b4c71c3a3dbe4a1f61ebdfdf14e095a24b307daa039dfb5201
  local active_mutation_started=false
  local completed=false

  # remove only invocation-owned activation evidence
  # shellcheck disable=SC2317,SC2329
  cleanup_normal_activation() {
    local status=$?
    local recovery_status=0
    trap - EXIT
    set +e
    rm -f -- "$pre_capacity_input" "$pre_capacity_receipt" \
      "$post_capacity_input" "$post_capacity_receipt"
    # classify only after the exact migration process can no longer commit
    if [[ "$completed" != true && "$active_mutation_started" == true ]]; then
      quiesce_normal_activation_migration "$activation_env" || recovery_status=1
      # shellcheck disable=SC2031
      start_postgres "$current_env" || recovery_status=1
      if [[ "$recovery_status" == 0 ]]; then
        # shellcheck disable=SC2031
        restore_normal_activation_after_quiescence \
          "$current_env" "$current" "$current" "$target" || recovery_status=1
      else
        recovery_status=1
      fi
      # remove authority only after exact-ledger restoration completes
      if [[ "$recovery_status" == 0 ]]; then
        rm -f -- "$normal_activation_journal" || recovery_status=1
        sync -f "$state_dir" || recovery_status=1
      fi
      if ((recovery_status != 0)); then
        printf 'Normal activation and exact-ledger restoration both failed.\n' >&2
        exit 1
      fi
    fi
    exit "$status"
  }
  activation_env=$(release_env "$target")
  validate_release_env "$activation_env" "$target"
  require_control_plane_compatibility "$activation_env"
  current=$(read_release_state "$state_dir/current-release")
  schema_release=$(read_release_state "$state_dir/schema-release")
  [[ "$current" == "$schema_release" && "$current" != "$target" ]] ||
    die "normal activation requires one exact forward staged release"
  current_env=$(normal_release_source_env)
  require_control_plane_compatibility "$current_env"
  stage_receipt="$releases_dir/$target.normal-stage-receipt.json"
  read_normal_stage_qualification "$current_env" "$activation_env" "$stage_receipt" >/dev/null
  authorization_path=$(migration_authorization "$target")
  validate_migration_authorization "$authorization_path" "$current" "$target"
  [[ ! -e "$normal_activation_journal" && ! -L "$normal_activation_journal" ]] ||
    die "normal activation recovery journal exists; run recover first"
  require_normal_release_resource_gate
  require_deployment_secrets

  pre_capacity_input=$(mktemp "$releases_dir/.${target}.XXXXXX.activation-pre-input")
  pre_capacity_receipt=$(mktemp "$releases_dir/.${target}.XXXXXX.activation-pre-receipt")
  post_capacity_input=$(mktemp "$releases_dir/.${target}.XXXXXX.activation-post-input")
  post_capacity_receipt=$(mktemp "$releases_dir/.${target}.XXXXXX.activation-post-receipt")
  chmod 600 "$pre_capacity_input" "$pre_capacity_receipt" \
    "$post_capacity_input" "$post_capacity_receipt"
  trap cleanup_normal_activation EXIT

  # recreate only PostgreSQL before measuring the exact backup boundary
  start_postgres "$current_env"
  require_normal_activation_capacity "$current_env" "$activation_env" "$stage_receipt" \
    pre_backup 0 "$pre_capacity_input" "$pre_capacity_receipt"
  backup_ceiling=$(node --input-type=module - "$pre_capacity_input" <<'NODE'
import { readFileSync } from "node:fs";
const input = JSON.parse(readFileSync(process.argv[2], "utf8"));
if (!Number.isSafeInteger(input.boundedBackupCeilingBytes) ||
  input.boundedBackupCeilingBytes < 1) throw new Error("backup ceiling is invalid");
process.stdout.write(String(input.boundedBackupCeilingBytes));
NODE
  )

  printf 'Creating pre-migration encrypted backup...\n'
  backup_output=$("$deploy_dir/scripts/backup.sh" --env-file "$current_env" \
    --max-output-bytes "$backup_ceiling") || die "pre-migration backup failed"
  printf '%s\n' "$backup_output"
  backup_archive=$(sed -n 's/^Backup complete: //p' <<<"$backup_output")
  backup_checksum=$(sed -n 's/^Checksum: //p' <<<"$backup_output")
  [[ -n "$backup_archive" && "$(grep -c '^Backup complete: ' <<<"$backup_output")" == 1 ]] ||
    die "bounded backup did not report one archive"
  [[ -n "$backup_checksum" && "$(grep -c '^Checksum: ' <<<"$backup_output")" == 1 &&
    "$backup_checksum" == "$backup_archive.sha256" ]] ||
    die "bounded backup did not report its adjacent checksum"
  require_canonical_descendant "$backup_archive" /var/lib/weather/backups \
    "bounded activation backup"
  require_canonical_descendant "$backup_checksum" /var/lib/weather/backups \
    "bounded activation backup checksum"
  [[ -f "$backup_archive" && ! -L "$backup_archive" ]] ||
    die "bounded activation backup is missing or linked"
  [[ -f "$backup_checksum" && ! -L "$backup_checksum" ]] ||
    die "bounded activation backup checksum is missing or linked"
  (cd /var/lib/weather/backups && sha256sum --check --status \
    "$(basename "$backup_checksum")") || die "bounded activation backup checksum differs"
  backup_bytes=$(stat -c '%s' "$backup_archive")
  require_normal_activation_capacity "$current_env" "$activation_env" "$stage_receipt" \
    post_backup "$backup_bytes" "$post_capacity_input" "$post_capacity_receipt"
  require_normal_activation_capacity_pair "$pre_capacity_receipt" \
    "$post_capacity_receipt" "$backup_archive" "$backup_checksum"

  # recheck every source and receipt binding at the irreversible schema boundary
  [[ "$(normal_release_source_env)" == "$current_env" ]] ||
    die "normal activation source changed after backup"
  validate_migration_authorization "$authorization_path" "$current" "$target"
  read_normal_stage_qualification "$current_env" "$activation_env" "$stage_receipt" >/dev/null
  require_normal_release_resource_gate

  # record schema intent before the first migration
  migration_container_name="weather_normal_migration_${target//./_}_$$"
  write_normal_activation_journal "$current" "$target" \
    "$migration_container_name" absent prepared
  active_mutation_started=true
  write_private_state "$state_dir/schema-release" "$target"
  printf 'Applying candidate migrations...\n'
  migration_container_id=$(WEATHER_ENV_FILE=$activation_env compose run --detach \
    --name "$migration_container_name" --no-deps migration \
    node deploy/scripts/migrate.mjs --atomic-normal-21-23)
  [[ "$migration_container_id" =~ ^[a-f0-9]{64}$ ]] ||
    die "normal activation migration container identity is invalid"
  write_normal_activation_journal "$current" "$target" \
    "$migration_container_name" "$migration_container_id" running
  migration_status=$(docker container wait "$migration_container_id") ||
    die "normal activation migration wait failed"
  if [[ "$migration_status" != 0 ]]; then
    docker container logs "$migration_container_id" >&2 || true
    die "normal activation migration failed"
  fi
  quiesce_normal_activation_migration "$activation_env" ||
    die "normal activation migration cleanup failed"
  actual_history=$(migration_history_sha256 \
    "$activation_env" "$(env_value "$activation_env" WEATHER_DATABASE_NAME)")
  [[ "$actual_history" == "$target_history" ]] ||
    die "normal activation migration ledger differs"
  apply_runtime_database_acl \
    "$activation_env" "$(env_value "$activation_env" WEATHER_DATABASE_NAME)"
  verify_runtime_database_acl \
    "$activation_env" "$(env_value "$activation_env" WEATHER_DATABASE_NAME)"

  printf 'Starting release %s...\n' "$target"
  if ! start_exact_release "$activation_env"; then
    die "release $target failed health checks"
  fi

  # record success only after every health gate
  record_release_success "$target" "$current"
  completed=true
  rm -f -- "$normal_activation_journal"
  sync -f "$state_dir"
  rm -f -- "$pre_capacity_input" "$pre_capacity_receipt" \
    "$post_capacity_input" "$post_capacity_receipt"
  trap - EXIT
  printf 'Release %s is active.\n' "$target"
)

# roll back images without migration
rollback_release() {
  local current previous schema_release current_env previous_env
  current=$(read_release_state "$state_dir/current-release")
  previous=$(read_release_state "$state_dir/previous-release")
  [[ "$current" != "$previous" ]] || die "previous release matches the active release"
  current_env=$(release_env "$current")
  previous_env=$(release_env "$previous")
  validate_release_env "$current_env" "$current"
  validate_release_env "$previous_env" "$previous"
  require_control_plane_compatibility "$current_env"
  require_control_plane_compatibility "$previous_env"
  require_deployment_secrets
  schema_release=$(read_optional_release_state "$state_dir/schema-release")

  # default legacy state to the active release
  if [[ -z "$schema_release" ]]; then
    schema_release=$current
  fi

  # switch only immutable runtime images
  if ! restore_images "$previous_env" "$previous" "$schema_release"; then
    restore_images "$current_env" "$current" "$schema_release" ||
      die "rollback and current-image recovery both failed"
    die "rollback failed; current Weather images were restored"
  fi

  record_release_success "$previous" "$current"
  printf 'Release %s is active after migration-free rollback.\n' "$previous"
}

# dispatch one operator action
main() {
(($# >= 1)) || { usage >&2; exit 2; }
action=$1
shift

case "$action" in
  # expose no caller-selected family, source, cutoff or SQL on the owner reader
  adjustment-confirmation-access-burn-v3|adjustment-shadow-terminal-record-v3|adjustment-shadow-terminal-retire-v3|adjustment-shadow-unsupported-terminal-record-v1|adjustment-shadow-unsupported-terminal-retire-v1)
    [[ "$#" == 1 ]] || die "adjustment owner operation requires one request identity"
    # select the fixed operation without accepting a function or SQL identifier
    case "$action" in
      adjustment-confirmation-access-burn-v3) run_adjustment_owner_operation_v3 access "$1" ;;
      adjustment-shadow-terminal-record-v3) run_adjustment_owner_operation_v3 terminal "$1" ;;
      adjustment-shadow-terminal-retire-v3) run_adjustment_owner_operation_v3 retire "$1" ;;
      adjustment-shadow-unsupported-terminal-record-v1) run_adjustment_owner_operation_v3 unsupported-terminal "$1" ;;
      adjustment-shadow-unsupported-terminal-retire-v1) run_adjustment_owner_operation_v3 unsupported-retire "$1" ;;
    esac
    ;;

  adjustment-shadow-metadata-custody-finalize-v1)
    [[ "$#" == 1 ]] || die "shadow metadata custody finalization requires one identity"
    finalize_adjustment_shadow_metadata_custody_v1 "$1"
    ;;

  adjustment-registration-lifecycle-status-v4)
    (($# == 0)) || die "registration lifecycle status takes no arguments"
    read_adjustment_registration_lifecycle_status_v4
    ;;
  adjustment-rain-fixed-gauge-target-sources-initialize-v1)
    (($# == 0)) || die "rain target source initialization takes no arguments"
    initialize_current_adjustment_rain_fixed_gauge_target_sources_v1
    ;;
  # accept only the authenticated create-once schedule identity over bounded stdin
  adjustment-registration-schedule-initialize-v3)
    (($# == 1)) || die "registration schedule initialization requires one hash"
    initialize_adjustment_registration_schedule_v3 "$1"
    ;;
  adjustment-family-release)
    (($# == 9)) || die "adjustment-family-release requires exactly nine arguments"
    target_release=$1
    compensating_release=$2
    expected_current_release=$3
    expected_source_release=$4
    expected_settings_sha256=$5
    family=$6
    action_sha256=$7
    report_sha256=$8
    fence=$9
    validate_release "$target_release"
    validate_release "$compensating_release"
    validate_release "$expected_current_release"
    validate_release "$expected_source_release"
    [[ "$expected_settings_sha256" =~ ^[a-f0-9]{64}$ &&
      "$action_sha256" =~ ^[a-f0-9]{64}$ && "$report_sha256" =~ ^[a-f0-9]{64}$ ]] ||
      die "adjustment-family-release hashes are invalid"
    [[ "$family" == temperature || "$family" == wind || "$family" == rain ]] ||
      die "adjustment-family-release family is invalid"
    [[ "$fence" =~ ^[1-9][0-9]{0,19}$ ]] ||
      die "adjustment-family-release fence is invalid"
    # compare fixed-width decimal strings without signed integer overflow
    # shellcheck disable=SC2071
    [[ ${#fence} -lt 20 || "$fence" < 18446744073709551616 ]] ||
      die "adjustment-family-release fence overflows uint64"
    [[ "$expected_current_release" == "$expected_source_release" &&
      "$target_release" != "$compensating_release" &&
      "$target_release" != "$expected_current_release" &&
      "$compensating_release" != "$expected_current_release" ]] ||
      die "adjustment-family-release release identities conflict"
    acquire_release_transaction_lock
    require_no_dashboard_override
    require_adjustment_family_fixed_roots
    require_adjustment_family_source_cas \
      "$expected_source_release" "$expected_settings_sha256"
    export WEATHER_RELEASE_LOCK_FD=$release_transaction_lock_fd
    require_command docker
    require_command node
    prepare_adjustment_family_release_pair "$target_release" "$compensating_release" \
      "$expected_source_release" "$family" "$action_sha256"
    exec node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" family-release "$@"
    ;;
  adjustment-family-apply-unlocked)
    (($# == 3)) || die "adjustment-family-apply-unlocked requires three releases"
    validate_release "$1"
    validate_release "$2"
    validate_release "$3"
    require_no_dashboard_override
    apply_adjustment_family_release_unlocked "$1" "$2" "$3"
    ;;
  dashboard-release)
    (($# == 3)) || die "dashboard-release requires target, core and web source releases"
    validate_release "$1"
    validate_release "$2"
    validate_release "$3"
    acquire_release_transaction_lock
    require_dashboard_fixed_roots
    require_command docker
    require_command node
    dashboard_release "$1" "$2" "$3"
    ;;
  dashboard-rollback)
    (($# == 0)) || die "dashboard-rollback takes no arguments"
    acquire_release_transaction_lock
    require_dashboard_fixed_roots
    require_command docker
    require_command node
    dashboard_rollback previous
    ;;
  dashboard-rollback-core)
    (($# == 0)) || die "dashboard-rollback-core takes no arguments"
    acquire_release_transaction_lock
    require_dashboard_fixed_roots
    require_command docker
    require_command node
    dashboard_rollback core
    ;;
  v14-compatibility-bridge)
    (($# == 1)) || die "v14-compatibility-bridge requires one immutable release"
    [[ "$EUID" == 0 ]] || die "v14-compatibility-bridge requires root"
    validate_release "$1"
    acquire_release_transaction_lock
    require_no_dashboard_override
    require_command docker
    require_command node
    v14_compatibility_bridge_release "$1"
    ;;
  historical-v14-18-21-rehearsal)
    (($# == 1)) || die "historical-v14-18-21-rehearsal requires one immutable release"
    [[ "$1" == "$full_v14_release" ]] ||
      die "historical-v14-18-21-rehearsal release differs"
    acquire_release_transaction_lock
    require_no_dashboard_override
    require_adjustment_family_fixed_roots
    require_command docker
    require_command node
    current=$(read_release_state "$state_dir/current-release")
    schema=$(read_release_state "$state_dir/schema-release")
    [[ "$current" == "$v14_compatibility_bridge_release" &&
      "$schema" == "$v14_compatibility_bridge_release" ]] ||
      die "historical rehearsal requires the exact committed schema-18 bridge"
    source_env=$(release_env "$current")
    require_v14_full_source_authority "$source_env"
    require_deployment_secrets
    node "$deploy_dir/scripts/adjustment-evaluation-package.mjs" \
      verify-full-v14-release "$1" >/dev/null ||
      die "historical rehearsal exact-commit CI proof failed"
    target=$(prepare_inert_v14_release "$1" "$source_env")
    historical_v14_18_21_rehearsal "$source_env" "$target"
    ;;
  inert-v14)
    (($# == 1)) || die "inert-v14 requires one immutable release"
    validate_release "$1"
    acquire_release_transaction_lock
    require_no_dashboard_override
    require_adjustment_family_fixed_roots
    require_command docker
    require_command node
    source_env=$(release_env "$v14_compatibility_bridge_release")
    require_file "$source_env"
    inert_v14_release "$1" "$source_env"
    ;;
  yolo)
    die "yolo is closed; use the staged normal release path"
    ;;
  stage)
    (($# == 1)) || die "stage requires exactly one release"
    release=$1
    validate_release "$release"
    acquire_release_transaction_lock
    require_no_dashboard_override
    source_env=$(normal_release_source_env)
    require_file "$source_env"
    require_command docker
    require_command node
    current=$(read_release_state "$state_dir/current-release")
    previous_env=$source_env
    require_normal_release_resource_gate
    mkdir -p "$releases_dir"
    target=$(release_env "$release")
    authorization=$(migration_authorization "$release")
    stage_receipt="$releases_dir/$release.normal-stage-receipt.json"
    stage_samples="$releases_dir/$release.normal-stage-samples.json"
    [[ ! -e "$target" && ! -L "$target" && ! -e "$authorization" &&
      ! -L "$authorization" && ! -e "$stage_receipt" && ! -L "$stage_receipt" &&
      ! -e "$stage_samples" && ! -L "$stage_samples" ]] ||
      die "release is already staged: $release"
    server_source="$(image_repository "$(env_value "$source_env" WEATHER_SERVER_IMAGE)"):$release"
    web_source="$(image_repository "$(env_value "$source_env" WEATHER_WEB_IMAGE)"):$release"
    server_image=$(resolve_arm64_image "$server_source")
    web_image=$(resolve_arm64_image "$web_source")
    postgres_image=$(resolve_arm64_image "$(env_value "$source_env" POSTGRES_IMAGE)")
    cloudflared_image=$(resolve_arm64_image "$(env_value "$source_env" CLOUDFLARED_IMAGE)")
    temporary=$(mktemp "$releases_dir/.${release}.XXXXXX.env.partial")
    temporary_authorization=
    published_authorization=
    pre_capacity_input=$(mktemp "$releases_dir/.${release}.XXXXXX.pre-capacity-input")
    pre_capacity_receipt=$(mktemp "$releases_dir/.${release}.XXXXXX.pre-capacity-receipt")
    post_capacity_input=$(mktemp "$releases_dir/.${release}.XXXXXX.post-capacity-input")
    post_capacity_receipt=$(mktemp "$releases_dir/.${release}.XXXXXX.post-capacity-receipt")
    migration_peak=$(mktemp "$releases_dir/.${release}.XXXXXX.migration-peak")
    temporary_stage_receipt=$(mktemp "$releases_dir/.${release}.XXXXXX.stage-receipt.partial")
    published_stage_receipt=
    published_stage_samples=
    chmod 600 "$pre_capacity_input" "$pre_capacity_receipt" "$post_capacity_input" \
      "$post_capacity_receipt" "$migration_peak" "$temporary_stage_receipt"

    # remove failed stage state
    trap 'rm -f "$temporary" "$pre_capacity_input" "$pre_capacity_receipt" "$post_capacity_input" "$post_capacity_receipt" "$migration_peak" "$temporary_stage_receipt" ${temporary_authorization:+"$temporary_authorization"} ${published_authorization:+"$published_authorization"} ${published_stage_receipt:+"$published_stage_receipt"} ${published_stage_samples:+"$published_stage_samples"}' EXIT
    write_release_env "$source_env" "$temporary" "$release" \
      "$server_image" "$web_image" "$postgres_image" "$cloudflared_image"
    WEATHER_ENV_FILE=$temporary compose config --quiet
    mapfile -t images < <(WEATHER_ENV_FILE=$temporary compose config --images | sort -u)
    ((${#images[@]} == 4)) || die "release must contain exactly four images"

    # reject any tag-only rendered image
    for image in "${images[@]}"; do
      validate_image_reference "$image"
    done

    require_normal_stage_capacity "$source_env" "$temporary" pre_pull \
      "$pre_capacity_input" "$pre_capacity_receipt"
    cleanup_obsolete_weather_images "$source_env" "$temporary"
    require_image_pull_capacity_floor
    WEATHER_ENV_FILE=$temporary compose pull
    require_normal_stage_capacity "$source_env" "$temporary" post_pull \
      "$post_capacity_input" "$post_capacity_receipt"

    # require the exact full-copy schema-21 to schema-23 compatibility proof
    require_deployment_secrets
    temporary_authorization=$(mktemp \
      "$releases_dir/.${release}.XXXXXX.migration-authorization.partial")
    verify_previous_image_compatibility \
      "$temporary" "$previous_env" "$temporary_authorization" normal-21-23 \
      "$migration_peak"

    # publish authorization before the release commit marker
    if [[ -n "$temporary_authorization" ]]; then
      publish_migration_authorization "$temporary_authorization" "$authorization"
      published_authorization=$authorization
      temporary_authorization=
    fi

    # publish the retained sample series before its cleanup-confirmed qualification
    ln "$migration_peak" "$stage_samples" ||
      die "normal stage capacity samples already exist or could not be published"
    sync -f "$stage_samples" "$releases_dir"
    published_stage_samples=$stage_samples

    # seal and publish the cleanup-confirmed stage qualification
    [[ "$(normal_release_source_env)" == "$source_env" ]] ||
      die "normal stage source changed during compatibility rehearsal"
    write_normal_stage_qualification_receipt "$source_env" "$temporary" \
      "$pre_capacity_receipt" "$post_capacity_receipt" "$migration_peak" \
      "$temporary_stage_receipt"
    read_normal_stage_qualification \
      "$source_env" "$temporary" "$temporary_stage_receipt" >/dev/null
    ln "$temporary_stage_receipt" "$stage_receipt" ||
      die "normal stage receipt already exists or could not be published"
    sync -f "$stage_receipt" "$releases_dir"
    published_stage_receipt=$stage_receipt
    rm -f -- "$temporary_stage_receipt"
    temporary_stage_receipt=

    # clean failed release publication
    if ! mv "$temporary" "$target"; then
      # remove orphaned authorization
      rm -f "$authorization" "$stage_receipt" "$stage_samples"
      exit 1
    fi
    published_authorization=
    published_stage_receipt=
    published_stage_samples=
    rm -f "$pre_capacity_input" "$pre_capacity_receipt" "$post_capacity_input" \
      "$post_capacity_receipt" "$migration_peak"
    trap - EXIT
    printf 'Release %s staged without changing running services or the active database.\n' "$release"
    ;;
  activate)
    (($# == 1)) || die "activate requires exactly one release"
    validate_release "$1"
    require_command age
    require_command docker
    acquire_release_transaction_lock
    require_no_dashboard_override
    start_release "$1"
    ;;
  rollback)
    (($# == 0)) || die "rollback takes no arguments"
    require_command docker
    acquire_release_transaction_lock
    require_no_dashboard_override
    rollback_release
    ;;
  recover)
    (($# == 0)) || die "recover takes no arguments"
    acquire_release_transaction_lock
    require_command docker
    require_command node
    dashboard_recovery=false
    dashboard_recovery_journal=false
    dashboard_recovery_expected_stable=
    dashboard_recovery_temporary=
    if dashboard_override_present; then
      dashboard_recovery=true
      require_dashboard_fixed_roots
    fi
    if [[ -e "$dashboard_release_journal" || -L "$dashboard_release_journal" ]]; then
      dashboard_recovery_journal=true
      validate_dashboard_release_journal
      dashboard_recovery_expected_stable=$(env_value \
        "$dashboard_release_journal" WEATHER_DASHBOARD_CORE_SNAPSHOT_SHA256)
      require_dashboard_journal_publication_proof
      reconcile_dashboard_release_journal
    fi
    current=$(read_release_state "$state_dir/current-release")
    schema_release=$(read_optional_release_state "$state_dir/schema-release")
    # close an interrupted normal migration before any ledger classification
    if [[ -e "$normal_activation_journal" || -L "$normal_activation_journal" ]]; then
      [[ "$dashboard_recovery" == false ]] ||
        die "dashboard and normal activation recovery authorities cannot coexist"
      read -r journal_source journal_target _ _ _ < <(read_normal_activation_journal)
      [[ ( "$current" == "$journal_source" || "$current" == "$journal_target" ) &&
        ( "$schema_release" == "$journal_source" || "$schema_release" == "$journal_target" ) ]] ||
        die "normal activation recovery markers differ from its journal"
      journal_source_env=$(release_env "$journal_source")
      journal_target_env=$(release_env "$journal_target")
      validate_release_env "$journal_source_env" "$journal_source"
      validate_release_env "$journal_target_env" "$journal_target"
      require_control_plane_compatibility "$journal_source_env"
      require_control_plane_compatibility "$journal_target_env"
      require_deployment_secrets
      quiesce_normal_activation_migration "$journal_target_env" ||
        die "interrupted normal migration could not be quiesced"
      current_env=$(release_env "$current")
      start_postgres "$current_env" || die "normal recovery PostgreSQL restart failed"
      restore_normal_activation_after_quiescence \
        "$current_env" "$current" "$journal_source" "$journal_target" ||
        die "normal activation recovery ledger is neither exact source nor target"
      write_active_symlink "$current"
      rm -f -- "$normal_activation_journal"
      sync -f "$state_dir"
      return
    fi
    # prefer exact normal-stage authority over the retained historical bridge receipt
    if [[ -n "$schema_release" &&
      ( -e "$releases_dir/$schema_release.normal-stage-receipt.json" ||
        -L "$releases_dir/$schema_release.normal-stage-receipt.json" ) ]]; then
      [[ "$dashboard_recovery" == false ]] ||
        die "dashboard and normal release recovery authorities cannot coexist"
      require_deployment_secrets
      recover_committed_normal_release "$current" "$schema_release"
      return
    fi
    v14_bridge_runtime_recovery=false
    v14_schema_marker_repair=false
    # classify the retained bridge receipt against exact runtime and schema markers
    if [[ -e "$v14_compatibility_bridge_state" || -L "$v14_compatibility_bridge_state" ]]; then
      [[ "$dashboard_recovery" == false ]] ||
        die "dashboard and v14 bridge recovery authorities cannot coexist"
      case "$current:$schema_release" in
        "$recurring_previous_release:$recurring_previous_release"|\
        "$recurring_previous_release:$v14_compatibility_bridge_release")
          # only an uncommitted bridge may restore source-one images and markers
          recover_v14_compatibility_bridge ||
            die "interrupted v14 compatibility bridge recovery failed"
          return
          ;;
        "$v14_compatibility_bridge_release:$v14_compatibility_bridge_release")
          # distinguish pristine schema 18 from a full-migration pre-marker crash
          require_v14_compatibility_bridge_receipt
          current_env=$(release_env "$v14_compatibility_bridge_release")
          if [[ "$(migration_history_sha256 "$current_env" \
              "$(env_value "$current_env" WEATHER_DATABASE_NAME)")" == \
            "$(env_value "$v14_compatibility_bridge_state" WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256)" ]]; then
            require_committed_v14_compatibility_bridge
          else
            require_v14_source2_full_schema_authority "$current_env"
            schema_release=$full_v14_release
            v14_schema_marker_repair=true
          fi
          v14_bridge_runtime_recovery=true
          ;;
        "$v14_compatibility_bridge_release:$full_v14_release")
          # restore source two only with exact full-schema authorization
          current_env=$(release_env "$v14_compatibility_bridge_release")
          require_v14_source2_full_schema_authority "$current_env"
          v14_bridge_runtime_recovery=true
          ;;
        "$full_v14_release:$full_v14_release")
          # restore the completed full release against its exact complete ledger
          require_v14_compatibility_bridge_receipt
          current_env=$(release_env "$full_v14_release")
          require_fixed_v14_migration_ledger \
            "$current_env" "$(env_value "$current_env" WEATHER_DATABASE_NAME)"
          require_full_v14_current_authority "$current_env"
          ;;
        *)
          die "v14 compatibility bridge recovery marker tuple is unsupported"
          ;;
      esac
    else
      # never recover a bridge or full runtime without its retained source authority
      if [[ "$current" == "$v14_compatibility_bridge_release" ||
        "$current" == "$full_v14_release" ||
        "$schema_release" == "$v14_compatibility_bridge_release" ||
        "$schema_release" == "$full_v14_release" ]]; then
        die "v14 recovery requires the retained compatibility bridge receipt"
      fi
    fi
    # default legacy state to the active release
    if [[ -z "$schema_release" ]]; then
      schema_release=$current
    fi

    # installed v14 must recover the pinned schema-18 core through its pre-epoch view
    if [[ "$current" == "$recurring_previous_release" &&
      "$schema_release" == "$recurring_previous_release" ]]; then
      dashboard_recovery=true
      require_dashboard_fixed_roots
    fi

    current_env=$(release_env "$current")
    validate_release_env "$current_env" "$current"
    require_control_plane_compatibility "$current_env"
    require_deployment_secrets
    if [[ "$dashboard_recovery" == true ]]; then
      [[ "$current" == "$recurring_previous_release" &&
        "$schema_release" == "$recurring_previous_release" ]] ||
        die "dashboard recovery core tuple differs"
      dashboard_recovery_temporary=$(mktemp -d \
        "${TMPDIR:-/tmp}/weather-dashboard-recovery.XXXXXX")
      trap 'rm -rf -- "$dashboard_recovery_temporary"' EXIT
      WEATHER_ENV_FILE=$current_env compose up -d --no-deps --no-recreate --wait postgres
      dashboard_recovery_full_before=$(dashboard_core_snapshot_sha256 \
        "$current_env" "$dashboard_recovery_temporary/core.before")
      dashboard_recovery_stable_before=$(dashboard_core_snapshot_sha256 \
        "$current_env" "$dashboard_recovery_temporary/core-stable.before" false)
      if [[ "$dashboard_recovery_journal" == true &&
        "$dashboard_recovery_stable_before" != "$dashboard_recovery_expected_stable" ]]; then
        die "dashboard recovery stable core proof differs from its journal"
      fi
    fi
    # keep source-two recovery independent of future-state bind files
    if [[ "$v14_bridge_runtime_recovery" == true ]]; then
      restore_v14_compatibility_bridge_images "$current_env" "$current" "$schema_release"
    elif [[ "$dashboard_recovery" == true ]]; then
      restore_dashboard_core_runtime "$current_env"
    else
      restore_images "$current_env" "$current" "$schema_release"
    fi
    # repair only a proven full-migration marker crash after source-two health
    if [[ "$v14_schema_marker_repair" == true ]]; then
      write_private_state "$state_dir/schema-release" "$full_v14_release"
    fi
    write_active_symlink "$current"
    if [[ "$dashboard_recovery" == true ]]; then
      dashboard_recovery_full_after=$(dashboard_core_snapshot_sha256 \
        "$current_env" "$dashboard_recovery_temporary/core.after")
      dashboard_recovery_stable_after=$(dashboard_core_snapshot_sha256 \
        "$current_env" "$dashboard_recovery_temporary/core-stable.after" false)
      [[ "$dashboard_recovery_full_after" == "$dashboard_recovery_full_before" &&
        "$dashboard_recovery_stable_after" == "$dashboard_recovery_stable_before" ]] ||
        die "dashboard recovery changed the stable core or non-web runtime"
      restore_dashboard_override_after_core_health "$current"
      if [[ "$dashboard_recovery_journal" == true ]]; then
        remove_dashboard_state "$dashboard_release_journal"
      fi
      rm -rf -- "$dashboard_recovery_temporary"
      dashboard_recovery_temporary=
      trap - EXIT
    fi
    ;;
  status)
    (($# == 0)) || die "status takes no arguments"
    exec "$deploy_dir/scripts/status.sh"
    ;;
  --help|-h)
    usage
    ;;
  *) die "unknown action: $action" ;;
esac
}

# run only from the release entrypoint
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
