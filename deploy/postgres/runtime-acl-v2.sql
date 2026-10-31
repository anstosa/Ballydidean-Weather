\set ON_ERROR_STOP on

SELECT format(
  'REVOKE ALL PRIVILEGES ON DATABASE %I FROM PUBLIC, weather_api, weather_ingest, weather_training_export',
  current_database()
)
\gexec
SELECT format(
  'GRANT CONNECT ON DATABASE %I TO weather_owner, weather_api, weather_ingest, weather_training_export',
  current_database()
)
\gexec

REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM weather_api, weather_ingest;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM weather_api, weather_ingest;
REVOKE ALL ON SCHEMA public FROM weather_api, weather_ingest;
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM weather_training_export;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM weather_training_export;
REVOKE ALL ON SCHEMA public FROM weather_training_export;

GRANT USAGE ON SCHEMA public TO weather_api, weather_ingest;
GRANT USAGE ON SCHEMA public TO weather_training_export;
GRANT SELECT ON sites, stations, providers, weather_records, worker_heartbeats, schema_migrations TO weather_api;
GRANT SELECT (
  id,
  station_id,
  provider_id,
  source_key,
  source_kind,
  capabilities,
  cadence_seconds,
  active,
  created_at,
  updated_at
) ON sources TO weather_api;
GRANT SELECT ON forecast_runtime_provenance_v1 TO weather_api;

GRANT SELECT ON sites, stations, providers, sources TO weather_ingest;
GRANT SELECT ON schema_migrations TO weather_ingest;
GRANT SELECT, INSERT, UPDATE ON ingestion_runs, ingestion_checkpoints, backfill_chunk_outcomes, worker_heartbeats TO weather_ingest;
GRANT SELECT, INSERT ON weather_records TO weather_ingest;
-- reconcile retained pre-canary clusters before forward migration
DO $canary_acl$
BEGIN
  -- grant sidecar authority only after both tables exist
  IF to_regclass('public.ecmwf_temperature_canary_runs') IS NOT NULL
    AND to_regclass('public.ecmwf_temperature_canary_hours') IS NOT NULL THEN
    GRANT SELECT ON ecmwf_temperature_canary_runs, ecmwf_temperature_canary_hours TO weather_api;
    GRANT SELECT, INSERT ON ecmwf_temperature_canary_runs, ecmwf_temperature_canary_hours TO weather_ingest;
    GRANT UPDATE (last_received_at) ON ecmwf_temperature_canary_runs TO weather_ingest;
  -- reject a partially installed sidecar schema
  ELSIF to_regclass('public.ecmwf_temperature_canary_runs') IS NOT NULL
    OR to_regclass('public.ecmwf_temperature_canary_hours') IS NOT NULL THEN
    RAISE EXCEPTION 'ECMWF temperature canary schema is incomplete';
  END IF;
END;
$canary_acl$;
-- grant prospective capture authority only when the complete private schema exists
DO $rain_capture_acl$
DECLARE
  rain_column record;
BEGIN
  IF to_regclass('public.rain_capture_claims') IS NOT NULL
    AND to_regclass('public.rain_capture_receipts') IS NOT NULL
    AND to_regclass('public.rain_collection_status_v1') IS NOT NULL THEN
    GRANT SELECT, INSERT ON rain_capture_claims, rain_capture_receipts TO weather_ingest;
    GRANT SELECT ON rain_collection_status_v1 TO weather_ingest, weather_api;
    REVOKE ALL ON rain_capture_claims, rain_capture_receipts FROM weather_api, weather_training_export;
    REVOKE ALL ON rain_collection_status_v1 FROM weather_training_export;
    -- clear historical column grants that table-level revocation does not remove
    FOR rain_column IN
      SELECT relation.relname AS table_name, attribute.attname AS column_name
      FROM pg_class relation
      JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
      JOIN pg_attribute attribute ON attribute.attrelid = relation.oid
      WHERE namespace.nspname = 'public'
        AND relation.relname IN (
          'rain_capture_claims', 'rain_capture_receipts', 'rain_collection_status_v1'
        )
        AND attribute.attnum > 0 AND NOT attribute.attisdropped
    LOOP
      EXECUTE format(
        'REVOKE ALL (%I) ON %I FROM weather_api, weather_ingest, weather_training_export',
        rain_column.column_name, rain_column.table_name
      );
    END LOOP;
  ELSIF to_regclass('public.rain_capture_claims') IS NOT NULL
    OR to_regclass('public.rain_capture_receipts') IS NOT NULL
    OR to_regclass('public.rain_collection_status_v1') IS NOT NULL THEN
    RAISE EXCEPTION 'prospective rain capture schema is incomplete';
  END IF;
END;
$rain_capture_acl$;
-- grant only the bounded inferred rain projection after its migration exists
DO $rain_adjustment_acl$
DECLARE
  adjustment_column record;
BEGIN
  -- retain compatibility with pre-adjustment rollback schemas
  IF to_regclass('public.rain_adjustment_runs') IS NOT NULL THEN
    GRANT SELECT ON rain_adjustment_runs TO weather_api, weather_ingest;
    GRANT INSERT ON rain_adjustment_runs TO weather_ingest;
    REVOKE ALL ON rain_adjustment_runs FROM weather_training_export;
    -- erase historical column grants before granting table-scoped authority
    FOR adjustment_column IN
      SELECT attribute.attname AS column_name
      FROM pg_attribute attribute
      WHERE attribute.attrelid = 'public.rain_adjustment_runs'::regclass
        AND attribute.attnum > 0 AND NOT attribute.attisdropped
    LOOP
      EXECUTE format(
        'REVOKE ALL (%I) ON rain_adjustment_runs FROM weather_api, weather_ingest, weather_training_export',
        adjustment_column.column_name
      );
    END LOOP;
  END IF;
END;
$rain_adjustment_acl$;
GRANT UPDATE (
  last_ingestion_run_id,
  last_received_at,
  upstream_timezone,
  upstream_model,
  device_vendor,
  device_model,
  device_serial,
  quality_metadata,
  provider_metadata,
  temperature_c,
  apparent_temperature_c,
  black_globe_temperature_c,
  precipitation_mm,
  precipitation_rate_mm_per_hour,
  wind_speed_mps,
  wind_gust_mps,
  pressure_hpa,
  relative_humidity_percent,
  cloud_cover_percent,
  wind_direction_degrees,
  pm25_micrograms_per_cubic_meter,
  soil_electrical_conductivity_us_cm,
  soil_moisture_percent,
  solar_radiation_wm2,
  uv_index,
  wet_bulb_globe_temperature_c,
  water_level_m,
  content_hash,
  revision_count
) ON weather_records TO weather_ingest;
GRANT INSERT ON forecast_anchor_records TO weather_ingest;
GRANT SELECT (
  source_id,
  source_kind,
  source_config_fingerprint,
  valid_at,
  lead_hours,
  dataset,
  upstream_model,
  contract_epoch,
  adapter_version,
  first_ingestion_run_id,
  last_ingestion_run_id,
  first_received_at,
  last_received_at,
  upstream_timezone,
  quality_metadata,
  provider_metadata,
  temperature_c,
  apparent_temperature_c,
  precipitation_mm,
  wind_speed_mps,
  wind_gust_mps,
  pressure_hpa,
  relative_humidity_percent,
  cloud_cover_percent,
  wind_direction_degrees,
  content_hash,
  revision_count
) ON forecast_anchor_records TO weather_ingest;
GRANT UPDATE (
  last_ingestion_run_id,
  last_received_at,
  quality_metadata,
  provider_metadata,
  temperature_c,
  apparent_temperature_c,
  precipitation_mm,
  wind_speed_mps,
  wind_gust_mps,
  pressure_hpa,
  relative_humidity_percent,
  cloud_cover_percent,
  wind_direction_degrees,
  content_hash,
  revision_count
) ON forecast_anchor_records TO weather_ingest;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO weather_ingest;

GRANT SELECT ON forecast_training_export_rows_v1 TO weather_training_export;
GRANT SELECT ON forecast_training_export_manifest_v1 TO weather_training_export;
-- grant the paired evaluation export only after its complete migration exists
DO $adjustment_evaluation_export_acl$
BEGIN
  -- retain compatibility while control v12 hands off schema 0016
  IF to_regclass('public.adjustment_evaluation_export_rows_v1') IS NOT NULL
    AND to_regclass('public.adjustment_evaluation_export_manifest_v1') IS NOT NULL THEN
    GRANT SELECT ON adjustment_evaluation_export_rows_v1 TO weather_training_export;
    GRANT SELECT ON adjustment_evaluation_export_manifest_v1 TO weather_training_export;
    REVOKE ALL ON adjustment_evaluation_export_rows_v1,
      adjustment_evaluation_export_manifest_v1 FROM weather_api, weather_ingest;
  -- reject a partially installed export contract
  ELSIF to_regclass('public.adjustment_evaluation_export_rows_v1') IS NOT NULL
    OR to_regclass('public.adjustment_evaluation_export_manifest_v1') IS NOT NULL THEN
    RAISE EXCEPTION 'adjustment evaluation export schema is incomplete';
  END IF;
END;
$adjustment_evaluation_export_acl$;

-- expose only the additive forecast product catalog when its full graph exists
DO $forecast_product_catalog_acl$
DECLARE
  catalog_column record;
  catalog_object_count integer;
BEGIN
  SELECT count(*) INTO catalog_object_count
  FROM (
    SELECT relation.oid
    FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relname = 'forecast_products'
    UNION ALL
    SELECT procedure.oid
    FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public'
      AND procedure.proname IN (
        'weather_sync_forecast_products_after_insert',
        'weather_sync_forecast_products_after_delete',
        'weather_sync_forecast_products_after_truncate'
      )
    UNION ALL
    SELECT trigger.oid
    FROM pg_trigger trigger
    JOIN pg_class relation ON relation.oid = trigger.tgrelid
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relname = 'weather_records'
      AND NOT trigger.tgisinternal
      AND trigger.tgname IN (
        'weather_records_forecast_products_insert',
        'weather_records_forecast_products_delete',
        'weather_records_forecast_products_truncate'
      )
  ) object;

  -- retain schema 0018 through 0021 only when every catalog object is absent
  IF catalog_object_count = 0 THEN
    IF EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0022_forecast_product_catalog.sql') THEN
      RAISE EXCEPTION 'forecast product catalog schema is partially absent';
    END IF;
    RETURN;
  END IF;
  -- reject a partial catalog before granting any reader
  IF catalog_object_count <> 7 OR NOT EXISTS (
    SELECT 1 FROM schema_migrations
    WHERE name = '0022_forecast_product_catalog.sql'
      AND checksum = '8d04170868414cd17e78cde2a4d6dd8144dfd606cba4e171686523f045cb77d2'
  ) THEN
    RAISE EXCEPTION 'forecast product catalog schema is incomplete';
  END IF;

  REVOKE ALL ON TABLE forecast_products
  FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  -- erase direct column grants before the table-scoped reader grant
  FOR catalog_column IN
    SELECT attribute.attname AS column_name
    FROM pg_attribute attribute
    WHERE attribute.attrelid = 'public.forecast_products'::regclass
      AND attribute.attnum > 0
      AND NOT attribute.attisdropped
  LOOP
    EXECUTE format(
      'REVOKE ALL (%I) ON forecast_products FROM PUBLIC, weather_api, weather_ingest, weather_training_export',
      catalog_column.column_name
    );
  END LOOP;
  REVOKE ALL ON FUNCTION weather_sync_forecast_products_after_insert(),
    weather_sync_forecast_products_after_delete(),
    weather_sync_forecast_products_after_truncate()
  FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  GRANT SELECT ON TABLE forecast_products TO weather_api;
END;
$forecast_product_catalog_acl$;

ALTER ROLE weather_training_export
  WITH LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
-- clear database-specific role defaults
SELECT format(
  'ALTER ROLE weather_training_export IN DATABASE %I RESET ALL',
  database.datname
)
FROM pg_db_role_setting setting
JOIN pg_database database ON database.oid = setting.setdatabase
WHERE setting.setrole = 'weather_training_export'::regrole
ORDER BY database.datname
\gexec
ALTER ROLE weather_training_export RESET ALL;
ALTER ROLE weather_training_export SET default_transaction_read_only = on;
SELECT format('REVOKE %I FROM weather_training_export', granted_role.rolname)
FROM pg_auth_members membership
JOIN pg_roles granted_role ON granted_role.oid = membership.roleid
WHERE membership.member = 'weather_training_export'::regrole
\gexec

-- remove inherited execution from application-owned and reachable definers
DO $$
DECLARE
  application_function record;
  restricted_schema record;
BEGIN
  -- enumerate only non-system application function identities
  FOR application_function IN
    SELECT procedure.oid::regprocedure AS identity
    FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
      AND namespace.nspname NOT LIKE 'pg_temp_%'
      AND namespace.nspname NOT LIKE 'pg_toast_temp_%'
      AND procedure.prosecdef
      AND (
        procedure.proowner = 'weather_owner'::regrole
        OR (
          (
            EXISTS (
              SELECT 1
              FROM aclexplode(coalesce(
                namespace.nspacl,
                acldefault('n', namespace.nspowner)
              )) schema_acl
              WHERE schema_acl.grantee = 0
                AND schema_acl.privilege_type = 'USAGE'
            )
            AND EXISTS (
              SELECT 1
              FROM aclexplode(coalesce(
                procedure.proacl,
                acldefault('f', procedure.proowner)
              )) function_acl
              WHERE function_acl.grantee = 0
                AND function_acl.privilege_type = 'EXECUTE'
            )
          )
          OR (
            has_schema_privilege('weather_api', namespace.oid, 'USAGE')
            AND has_function_privilege('weather_api', procedure.oid, 'EXECUTE')
          )
          OR (
            has_schema_privilege('weather_ingest', namespace.oid, 'USAGE')
            AND has_function_privilege('weather_ingest', procedure.oid, 'EXECUTE')
          )
          OR (
            has_schema_privilege('weather_training_export', namespace.oid, 'USAGE')
            AND has_function_privilege(
              'weather_training_export', procedure.oid, 'EXECUTE'
            )
          )
        )
      )
    ORDER BY namespace.nspname, procedure.oid::regprocedure::text
  LOOP
    EXECUTE format(
      'REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC, weather_api, weather_ingest, weather_training_export',
      application_function.identity
    );
  END LOOP;

  -- remove direct export access from every non-system auxiliary namespace
  FOR restricted_schema IN
    SELECT namespace.nspname
    FROM pg_namespace namespace
    WHERE namespace.nspname <> 'public'
      AND namespace.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
      AND namespace.nspname NOT LIKE 'pg_temp_%'
      AND namespace.nspname NOT LIKE 'pg_toast_temp_%'
    ORDER BY namespace.nspname
  LOOP
    EXECUTE format(
      'REVOKE ALL ON SCHEMA %I FROM weather_training_export',
      restricted_schema.nspname
    );
    -- remove ambient DDL without removing function reachability
    EXECUTE format(
      'REVOKE CREATE ON SCHEMA %I FROM PUBLIC',
      restricted_schema.nspname
    );
  END LOOP;
END;
$$;

GRANT EXECUTE ON FUNCTION weather_source_is_current(bigint)
TO weather_api, weather_ingest;
GRANT EXECUTE ON FUNCTION weather_json_object_keys_allowed(jsonb, text[])
TO weather_ingest;

-- reconcile the optional all-or-nothing adjustment maintenance v2 graph
DO $adjustment_maintenance_v2_acl$
DECLARE
  present_object_count integer;
  exact_object_count integer;
  maintenance_column record;
BEGIN
  SELECT count(*) INTO present_object_count
  FROM (
    SELECT relation.oid
    FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relname IN (
        'adjustment_shadow_registrations_v2',
        'adjustment_shadow_predictions_v2',
        'adjustment_confirmation_accesses_v2'
      )
    UNION ALL
    SELECT procedure.oid
    FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public'
      AND procedure.proname IN (
        'weather_guard_adjustment_shadow_registration_v2',
        'weather_guard_adjustment_shadow_prediction_v2',
        'weather_guard_adjustment_confirmation_access_v2',
        'weather_reject_adjustment_maintenance_mutation',
        'weather_register_adjustment_shadow_v2',
        'weather_append_adjustment_temperature_shadow_v2',
        'weather_append_adjustment_wind_shadow_v2',
        'weather_append_adjustment_rain_shadow_v2',
        'adjustment_shadow_body_admission_v2',
        'weather_finalize_adjustment_shadow_metadata_v2',
        'weather_record_adjustment_confirmation_access_v2',
        'adjustment_confirmation_availability_v2',
        'adjustment_confirmation_export_v2'
      )
  ) present;

  -- preserve previous images only when every v2 object is absent
  IF present_object_count = 0 THEN
    RETURN;
  END IF;

  SELECT count(*) INTO exact_object_count
  FROM (VALUES
    (to_regclass('public.adjustment_shadow_registrations_v2')::oid),
    (to_regclass('public.adjustment_shadow_predictions_v2')::oid),
    (to_regclass('public.adjustment_confirmation_accesses_v2')::oid),
    (to_regprocedure('public.weather_guard_adjustment_shadow_registration_v2()')::oid),
    (to_regprocedure('public.weather_guard_adjustment_shadow_prediction_v2()')::oid),
    (to_regprocedure('public.weather_guard_adjustment_confirmation_access_v2()')::oid),
    (to_regprocedure('public.weather_reject_adjustment_maintenance_mutation()')::oid),
    (to_regprocedure('public.weather_register_adjustment_shadow_v2(jsonb)')::oid),
    (to_regprocedure('public.weather_append_adjustment_temperature_shadow_v2(jsonb)')::oid),
    (to_regprocedure('public.weather_append_adjustment_wind_shadow_v2(jsonb)')::oid),
    (to_regprocedure('public.weather_append_adjustment_rain_shadow_v2(jsonb)')::oid),
    (to_regprocedure('public.adjustment_shadow_body_admission_v2(text,text,text,integer)')::oid),
    (to_regprocedure('public.weather_finalize_adjustment_shadow_metadata_v2(jsonb)')::oid),
    (to_regprocedure('public.weather_record_adjustment_confirmation_access_v2(jsonb)')::oid),
    (to_regprocedure('public.adjustment_confirmation_availability_v2(text)')::oid),
    (to_regprocedure('public.adjustment_confirmation_export_v2(text,text,smallint)')::oid)
  ) expected(oid)
  WHERE oid IS NOT NULL;
  -- reject every partial or wrong-signature graph
  IF present_object_count <> 16 OR exact_object_count <> 16 THEN
    RAISE EXCEPTION 'adjustment maintenance v2 schema is incomplete';
  END IF;
  -- require exactly four trigger functions and nine closed APIs
  IF (SELECT count(*)
      FROM pg_proc procedure
      JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
      WHERE namespace.nspname = 'public'
        AND procedure.proname IN (
          'weather_guard_adjustment_shadow_registration_v2',
          'weather_guard_adjustment_shadow_prediction_v2',
          'weather_guard_adjustment_confirmation_access_v2',
          'weather_reject_adjustment_maintenance_mutation',
          'weather_register_adjustment_shadow_v2',
          'weather_append_adjustment_temperature_shadow_v2',
          'weather_append_adjustment_wind_shadow_v2',
          'weather_append_adjustment_rain_shadow_v2',
          'adjustment_shadow_body_admission_v2',
          'weather_finalize_adjustment_shadow_metadata_v2',
          'weather_record_adjustment_confirmation_access_v2',
          'adjustment_confirmation_availability_v2',
          'adjustment_confirmation_export_v2'
        )) <> 13 THEN
    RAISE EXCEPTION 'adjustment maintenance v2 function graph is not exact';
  END IF;
  -- require owner-controlled tables and functions
  IF EXISTS (
    SELECT 1
    FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relname IN (
        'adjustment_shadow_registrations_v2',
        'adjustment_shadow_predictions_v2',
        'adjustment_confirmation_accesses_v2'
      )
      AND relation.relowner <> 'weather_owner'::regrole
  ) OR EXISTS (
    SELECT 1
    FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public'
      AND procedure.proname IN (
        'weather_guard_adjustment_shadow_registration_v2',
        'weather_guard_adjustment_shadow_prediction_v2',
        'weather_guard_adjustment_confirmation_access_v2',
        'weather_reject_adjustment_maintenance_mutation',
        'weather_register_adjustment_shadow_v2',
        'weather_append_adjustment_temperature_shadow_v2',
        'weather_append_adjustment_wind_shadow_v2',
        'weather_append_adjustment_rain_shadow_v2',
        'adjustment_shadow_body_admission_v2',
        'weather_finalize_adjustment_shadow_metadata_v2',
        'weather_record_adjustment_confirmation_access_v2',
        'adjustment_confirmation_availability_v2',
        'adjustment_confirmation_export_v2'
      )
      AND procedure.proowner <> 'weather_owner'::regrole
  ) THEN
    RAISE EXCEPTION 'adjustment maintenance v2 ownership is invalid';
  END IF;
  -- require the nine closed APIs to remain hardened definers
  IF EXISTS (
    SELECT 1
    FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public'
      AND procedure.proname IN (
        'weather_register_adjustment_shadow_v2',
        'weather_append_adjustment_temperature_shadow_v2',
        'weather_append_adjustment_wind_shadow_v2',
        'weather_append_adjustment_rain_shadow_v2',
        'adjustment_shadow_body_admission_v2',
        'weather_finalize_adjustment_shadow_metadata_v2',
        'weather_record_adjustment_confirmation_access_v2',
        'adjustment_confirmation_availability_v2',
        'adjustment_confirmation_export_v2'
      )
      AND (NOT procedure.prosecdef
        OR procedure.proconfig IS NULL
        OR NOT (procedure.proconfig @> ARRAY['search_path=pg_catalog, public']))
  ) THEN
    RAISE EXCEPTION 'adjustment maintenance v2 function hardening is invalid';
  END IF;
  -- require exact stable and volatile classifications
  IF EXISTS (
    SELECT 1
    FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public'
      AND (
        (procedure.proname IN (
          'adjustment_shadow_body_admission_v2',
          'adjustment_confirmation_availability_v2',
          'adjustment_confirmation_export_v2'
        ) AND procedure.provolatile <> 's')
        OR
        (procedure.proname IN (
          'weather_register_adjustment_shadow_v2',
          'weather_append_adjustment_temperature_shadow_v2',
          'weather_append_adjustment_wind_shadow_v2',
          'weather_append_adjustment_rain_shadow_v2',
          'weather_finalize_adjustment_shadow_metadata_v2',
          'weather_record_adjustment_confirmation_access_v2'
        ) AND procedure.provolatile <> 'v')
      )
  ) THEN
    RAISE EXCEPTION 'adjustment maintenance v2 volatility is invalid';
  END IF;
  -- require every expected mutation trigger binding
  IF (SELECT count(*)
      FROM pg_trigger trigger
      JOIN pg_class relation ON relation.oid = trigger.tgrelid
      JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public'
        AND relation.relname IN (
          'adjustment_shadow_registrations_v2',
          'adjustment_shadow_predictions_v2',
          'adjustment_confirmation_accesses_v2'
        )
        AND NOT trigger.tgisinternal
        AND trigger.tgname IN (
          'adjustment_shadow_registrations_v2_guard_insert',
          'adjustment_shadow_predictions_v2_guard_insert',
          'adjustment_confirmation_accesses_v2_guard_insert',
          'adjustment_shadow_registrations_v2_guard_mutation',
          'adjustment_shadow_predictions_v2_guard_mutation',
          'adjustment_confirmation_accesses_v2_guard_mutation',
          'adjustment_shadow_registrations_v2_guard_truncate',
          'adjustment_shadow_predictions_v2_guard_truncate',
          'adjustment_confirmation_accesses_v2_guard_truncate'
        )) <> 9 THEN
    RAISE EXCEPTION 'adjustment maintenance v2 trigger graph is incomplete';
  END IF;

  REVOKE ALL ON adjustment_shadow_registrations_v2,
    adjustment_shadow_predictions_v2,
    adjustment_confirmation_accesses_v2
  FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  -- erase historical column grants before function-only grants
  FOR maintenance_column IN
    SELECT relation.relname AS table_name, attribute.attname AS column_name
    FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    JOIN pg_attribute attribute ON attribute.attrelid = relation.oid
    WHERE namespace.nspname = 'public'
      AND relation.relname IN (
        'adjustment_shadow_registrations_v2',
        'adjustment_shadow_predictions_v2',
        'adjustment_confirmation_accesses_v2'
      )
      AND attribute.attnum > 0
      AND NOT attribute.attisdropped
  LOOP
    EXECUTE format(
      'REVOKE ALL (%I) ON %I FROM weather_api, weather_ingest, weather_training_export',
      maintenance_column.column_name,
      maintenance_column.table_name
    );
  END LOOP;

  GRANT EXECUTE ON FUNCTION weather_register_adjustment_shadow_v2(jsonb)
  TO weather_api, weather_ingest;
  GRANT EXECUTE ON FUNCTION weather_append_adjustment_temperature_shadow_v2(jsonb),
    weather_append_adjustment_wind_shadow_v2(jsonb)
  TO weather_api;
  GRANT EXECUTE ON FUNCTION weather_append_adjustment_rain_shadow_v2(jsonb)
  TO weather_ingest;
  GRANT EXECUTE ON FUNCTION adjustment_shadow_body_admission_v2(text,text,text,integer)
  TO weather_api;
  GRANT EXECUTE ON FUNCTION adjustment_confirmation_availability_v2(text),
    adjustment_confirmation_export_v2(text,text,smallint)
  TO weather_training_export;
END;
$adjustment_maintenance_v2_acl$;

-- reconcile the additive recurring schema without granting a new runtime API
DO $adjustment_maintenance_recurring_acl$
DECLARE
  object_count integer;
  exact_count integer;
  maintenance_column record;
BEGIN
  SELECT count(*) INTO object_count FROM (
    SELECT relation.oid FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relname = 'adjustment_shadow_terminal_results_v2'
    UNION ALL
    SELECT procedure.oid FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public' AND procedure.proname IN (
      'weather_append_adjustment_shadow_v3',
      'weather_guard_adjustment_shadow_terminal_result_v2',
      'weather_record_adjustment_shadow_terminal_result_v2',
      'weather_retire_adjustment_shadow_registration_v2'
    )
  ) objects;
  -- preserve the complete pre-recurring schema only when the entire annex is absent
  IF object_count = 0 THEN
    IF EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0019_adjustment_maintenance_recurring.sql')
      OR EXISTS (SELECT 1 FROM pg_attribute
        WHERE attrelid = to_regclass('public.adjustment_shadow_predictions_v2')
          AND attname IN ('candidate_sha256', 'source_sha256',
            'scored_min_valid_at', 'scored_max_valid_at')
          AND NOT attisdropped) THEN
      RAISE EXCEPTION 'recurring maintenance schema is partially absent';
    END IF;
    RETURN;
  END IF;
  SELECT count(*) INTO exact_count FROM (VALUES
    (to_regclass('public.adjustment_shadow_terminal_results_v2')::oid),
    (to_regprocedure('public.weather_append_adjustment_shadow_v3(jsonb,text)')::oid),
    (to_regprocedure('public.weather_guard_adjustment_shadow_terminal_result_v2()')::oid),
    (to_regprocedure('public.weather_record_adjustment_shadow_terminal_result_v2(jsonb)')::oid),
    (to_regprocedure('public.weather_retire_adjustment_shadow_registration_v2(jsonb)')::oid)
  ) expected(oid) WHERE oid IS NOT NULL;
  -- refuse partial objects and extra overloads rather than repairing their identities
  IF object_count <> 5 OR exact_count <> 5
    OR NOT EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0019_adjustment_maintenance_recurring.sql') THEN
    RAISE EXCEPTION 'recurring maintenance schema is incomplete';
  END IF;
  -- retain only owner-controlled hardened functions and one value-free table
  IF (SELECT relowner FROM pg_class
      WHERE oid = 'public.adjustment_shadow_terminal_results_v2'::regclass)
      <> 'weather_owner'::regrole
    OR EXISTS (SELECT 1 FROM pg_proc procedure
      JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
      WHERE namespace.nspname = 'public' AND procedure.proname IN (
        'weather_append_adjustment_shadow_v3',
        'weather_guard_adjustment_shadow_terminal_result_v2',
        'weather_record_adjustment_shadow_terminal_result_v2',
        'weather_retire_adjustment_shadow_registration_v2'
      ) AND (procedure.proowner <> 'weather_owner'::regrole
        OR procedure.provolatile <> 'v'
        OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public']
        OR procedure.prosecdef IS DISTINCT FROM
          (procedure.proname <> 'weather_guard_adjustment_shadow_terminal_result_v2'))) THEN
    RAISE EXCEPTION 'recurring maintenance owner or function hardening differs';
  END IF;
  REVOKE ALL ON adjustment_shadow_terminal_results_v2
    FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  REVOKE EXECUTE ON FUNCTION weather_append_adjustment_shadow_v3(jsonb,text),
    weather_guard_adjustment_shadow_terminal_result_v2(),
    weather_record_adjustment_shadow_terminal_result_v2(jsonb),
    weather_retire_adjustment_shadow_registration_v2(jsonb)
    FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  -- remove historical column grants without adding direct metadata access
  FOR maintenance_column IN SELECT attname FROM pg_attribute
    WHERE attrelid = 'public.adjustment_shadow_terminal_results_v2'::regclass
      AND attnum > 0 AND NOT attisdropped
  LOOP
    EXECUTE format('REVOKE ALL (%I) ON adjustment_shadow_terminal_results_v2 FROM PUBLIC, weather_api, weather_ingest, weather_training_export',
      maintenance_column.attname);
  END LOOP;
END;
$adjustment_maintenance_recurring_acl$;

-- reconcile the bounded revision-frontier annex without widening runtime writes
DO $adjustment_revision_frontier_acl$
DECLARE
  object_count integer;
  exact_count integer;
  revision_column record;
  serving_grant record;
  serving_columns text;
BEGIN
  SELECT count(*) INTO object_count FROM (
    SELECT relation.oid FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relname IN ('adjustment_revision_frontier_v1',
        'adjustment_revision_ordinal_v1')
    UNION ALL
    SELECT procedure.oid FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public' AND procedure.proname IN (
      'weather_append_adjustment_shadow_v4', 'adjustment_revision_frontier_v1',
      'adjustment_shadow_revision_admission_v1',
      'weather_adjustment_revision_receipt_valid_v1',
      'weather_clear_adjustment_revision_pointer_v1',
      'weather_issue_adjustment_revision_receipt_v1',
      'weather_bind_weather_record_revisions_v1',
      'weather_bind_forecast_anchor_revisions_v1',
      'weather_guard_rain_adjustment_revision_v1',
      'weather_bind_rain_gate_revision_v1',
      'weather_bind_ecmwf_temperature_revision_v1',
      'adjustment_revision_serving_snapshot_v1',
      'adjustment_weather_revision_admission_v1',
      'adjustment_forecast_anchor_revision_admission_v1',
      'adjustment_rain_gate_revision_admission_v1',
      'adjustment_ecmwf_temperature_revision_admission_v1'
      ,'weather_mark_adjustment_revision_gap_v1'
    )
  ) objects;
  -- preserve the complete pre-frontier schema only when the annex is absent
  IF object_count = 0 THEN
    IF EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0020_adjustment_revision_frontier.sql')
      OR EXISTS (SELECT 1 FROM pg_attribute
        WHERE attrelid = to_regclass('public.adjustment_shadow_predictions_v2')
          AND attname = 'stage_receipt_sha256' AND NOT attisdropped) THEN
      RAISE EXCEPTION 'adjustment revision frontier schema is partially absent';
    END IF;
    RETURN;
  END IF;
  SELECT count(*) INTO exact_count FROM (VALUES
    (to_regclass('public.adjustment_revision_frontier_v1')::oid),
    (to_regclass('public.adjustment_revision_ordinal_v1')::oid),
    (to_regprocedure('public.weather_append_adjustment_shadow_v4(jsonb,text)')::oid),
    (to_regprocedure('public.adjustment_revision_frontier_v1()')::oid),
    (to_regprocedure('public.adjustment_shadow_revision_admission_v1(text,text,text,integer)')::oid),
    (to_regprocedure('public.weather_adjustment_revision_receipt_valid_v1(jsonb)')::oid),
    (to_regprocedure('public.weather_clear_adjustment_revision_pointer_v1()')::oid),
    (to_regprocedure('public.weather_issue_adjustment_revision_receipt_v1(text,text,text,text)')::oid),
    (to_regprocedure('public.weather_bind_weather_record_revisions_v1(jsonb)')::oid),
    (to_regprocedure('public.weather_bind_forecast_anchor_revisions_v1(jsonb)')::oid),
    (to_regprocedure('public.weather_guard_rain_adjustment_revision_v1()')::oid),
    (to_regprocedure('public.weather_bind_rain_gate_revision_v1(jsonb)')::oid),
    (to_regprocedure('public.weather_bind_ecmwf_temperature_revision_v1(jsonb)')::oid),
    (to_regprocedure('public.adjustment_revision_serving_snapshot_v1(timestamptz,bigint)')::oid)
    ,(to_regprocedure('public.adjustment_weather_revision_admission_v1(jsonb)')::oid)
    ,(to_regprocedure('public.adjustment_forecast_anchor_revision_admission_v1(jsonb)')::oid)
    ,(to_regprocedure('public.adjustment_rain_gate_revision_admission_v1(jsonb)')::oid)
    ,(to_regprocedure('public.adjustment_ecmwf_temperature_revision_admission_v1(jsonb)')::oid)
    ,(to_regprocedure('public.weather_mark_adjustment_revision_gap_v1(text,jsonb,jsonb)')::oid)
  ) expected(oid) WHERE oid IS NOT NULL;
  -- refuse partial objects and extra overloads rather than repairing identities
  IF object_count <> 19 OR exact_count <> 19
    OR NOT EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0020_adjustment_revision_frontier.sql')
    OR (SELECT count(*) FROM pg_attribute
      WHERE attrelid = 'public.adjustment_shadow_predictions_v2'::regclass
        AND attname IN ('stage_receipt_sha256', 'archive_commit_ordinal',
          'predecessor_frontier_sha256', 'revision_frontier_sha256',
          'revision_receipt_sha256', 'archive_committed_at') AND NOT attisdropped) <> 6
    OR EXISTS (SELECT 1 FROM (VALUES ('weather_records'), ('forecast_anchor_records'),
        ('rain_adjustment_runs'), ('ecmwf_temperature_canary_runs')) expected(name)
      WHERE NOT EXISTS (SELECT 1 FROM pg_attribute attribute
        WHERE attribute.attrelid = to_regclass('public.' || expected.name)
          AND attribute.attname = 'adjustment_revision_receipt'
          AND NOT attribute.attisdropped)) THEN
    RAISE EXCEPTION 'adjustment revision frontier schema is incomplete';
  END IF;
  -- retain exact owner-controlled object identities and hardening
  IF EXISTS (SELECT 1 FROM pg_class relation
      WHERE relation.oid IN ('public.adjustment_revision_frontier_v1'::regclass,
        'public.adjustment_revision_ordinal_v1'::regclass)
        AND relation.relowner <> 'weather_owner'::regrole)
    OR EXISTS (SELECT 1 FROM pg_proc procedure
      JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
      WHERE namespace.nspname = 'public'
        AND procedure.proname IN ('weather_append_adjustment_shadow_v4',
          'adjustment_revision_frontier_v1', 'adjustment_shadow_revision_admission_v1',
          'weather_adjustment_revision_receipt_valid_v1',
          'weather_clear_adjustment_revision_pointer_v1',
          'weather_issue_adjustment_revision_receipt_v1',
          'weather_bind_weather_record_revisions_v1',
          'weather_bind_forecast_anchor_revisions_v1',
          'weather_guard_rain_adjustment_revision_v1',
          'weather_bind_rain_gate_revision_v1',
          'weather_bind_ecmwf_temperature_revision_v1',
          'adjustment_revision_serving_snapshot_v1',
          'adjustment_weather_revision_admission_v1',
          'adjustment_forecast_anchor_revision_admission_v1',
          'adjustment_rain_gate_revision_admission_v1',
          'adjustment_ecmwf_temperature_revision_admission_v1',
          'weather_mark_adjustment_revision_gap_v1')
        AND (procedure.proowner <> 'weather_owner'::regrole
          OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public']))
    OR EXISTS (SELECT 1 FROM pg_proc procedure
      JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
      WHERE namespace.nspname = 'public'
        AND procedure.proname IN ('weather_append_adjustment_shadow_v4',
          'adjustment_revision_frontier_v1', 'adjustment_shadow_revision_admission_v1',
          'weather_bind_weather_record_revisions_v1',
          'weather_bind_forecast_anchor_revisions_v1',
          'weather_bind_rain_gate_revision_v1',
          'weather_bind_ecmwf_temperature_revision_v1',
          'adjustment_revision_serving_snapshot_v1',
          'adjustment_weather_revision_admission_v1',
          'adjustment_forecast_anchor_revision_admission_v1',
          'adjustment_rain_gate_revision_admission_v1',
          'adjustment_ecmwf_temperature_revision_admission_v1',
          'weather_mark_adjustment_revision_gap_v1')
        AND procedure.prosecdef IS NOT true) THEN
    RAISE EXCEPTION 'adjustment revision frontier owner or function hardening differs';
  END IF;
  REVOKE ALL ON TABLE adjustment_revision_frontier_v1
    FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  REVOKE ALL ON SEQUENCE adjustment_revision_ordinal_v1
    FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  REVOKE EXECUTE ON FUNCTION weather_append_adjustment_shadow_v4(jsonb,text),
    weather_adjustment_revision_receipt_valid_v1(jsonb),
    weather_clear_adjustment_revision_pointer_v1(),
    weather_issue_adjustment_revision_receipt_v1(text,text,text,text),
    weather_bind_weather_record_revisions_v1(jsonb),
    weather_bind_forecast_anchor_revisions_v1(jsonb),
    weather_guard_rain_adjustment_revision_v1(),
    weather_bind_rain_gate_revision_v1(jsonb),
    weather_bind_ecmwf_temperature_revision_v1(jsonb),
    adjustment_revision_frontier_v1(),
    adjustment_shadow_revision_admission_v1(text,text,text,integer),
    adjustment_revision_serving_snapshot_v1(timestamptz,bigint)
    ,adjustment_weather_revision_admission_v1(jsonb)
    ,adjustment_forecast_anchor_revision_admission_v1(jsonb)
    ,adjustment_rain_gate_revision_admission_v1(jsonb)
    ,adjustment_ecmwf_temperature_revision_admission_v1(jsonb)
    ,weather_mark_adjustment_revision_gap_v1(text,jsonb,jsonb)
    FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  GRANT EXECUTE ON FUNCTION adjustment_revision_frontier_v1(),
    adjustment_revision_serving_snapshot_v1(timestamptz,bigint)
    TO weather_training_export;
  GRANT EXECUTE ON FUNCTION adjustment_shadow_revision_admission_v1(text,text,text,integer)
    TO weather_api;
  GRANT EXECUTE ON FUNCTION adjustment_weather_revision_admission_v1(jsonb),
    adjustment_forecast_anchor_revision_admission_v1(jsonb),
    adjustment_rain_gate_revision_admission_v1(jsonb),
    adjustment_ecmwf_temperature_revision_admission_v1(jsonb)
    TO weather_api;
  GRANT EXECUTE ON FUNCTION weather_bind_weather_record_revisions_v1(jsonb),
    weather_bind_forecast_anchor_revisions_v1(jsonb),
    weather_bind_rain_gate_revision_v1(jsonb),
    weather_bind_ecmwf_temperature_revision_v1(jsonb)
    TO weather_ingest;
  GRANT EXECUTE ON FUNCTION weather_mark_adjustment_revision_gap_v1(text,jsonb,jsonb)
    TO weather_api, weather_ingest;
  -- permit only the pure check predicate needed by constrained serving writes
  GRANT EXECUTE ON FUNCTION weather_adjustment_revision_receipt_valid_v1(jsonb)
    TO weather_api, weather_ingest;
  -- replace table-wide grants so future pointer columns stay private
  FOR serving_grant IN SELECT * FROM (VALUES
      ('weather_records', 'weather_api', 'SELECT'),
      ('weather_records', 'weather_ingest', 'SELECT'),
      ('weather_records', 'weather_ingest', 'INSERT'),
      ('forecast_anchor_records', 'weather_ingest', 'INSERT'),
      ('rain_adjustment_runs', 'weather_api', 'SELECT'),
      ('rain_adjustment_runs', 'weather_ingest', 'SELECT'),
      ('rain_adjustment_runs', 'weather_ingest', 'INSERT'),
      ('ecmwf_temperature_canary_runs', 'weather_api', 'SELECT'),
      ('ecmwf_temperature_canary_runs', 'weather_ingest', 'SELECT'),
      ('ecmwf_temperature_canary_runs', 'weather_ingest', 'INSERT')
    ) grant_spec(relation_name, role_name, privilege_name)
  LOOP
    SELECT string_agg(format('%I', attribute.attname), ', ' ORDER BY attribute.attnum)
    INTO STRICT serving_columns FROM pg_attribute attribute
    WHERE attribute.attrelid = to_regclass('public.' || serving_grant.relation_name)
      AND attribute.attnum > 0 AND NOT attribute.attisdropped
      AND attribute.attname <> 'adjustment_revision_receipt';
    EXECUTE format('REVOKE %s ON %I FROM %I', serving_grant.privilege_name,
      serving_grant.relation_name, serving_grant.role_name);
    EXECUTE format('GRANT %s (%s) ON %I TO %I', serving_grant.privilege_name,
      serving_columns, serving_grant.relation_name, serving_grant.role_name);
  END LOOP;
  -- remove direct access to every new per-row receipt field
  FOR revision_column IN
    SELECT relation.name AS relation_name, attribute.attname FROM (VALUES
      ('adjustment_shadow_predictions_v2'), ('weather_records'),
      ('forecast_anchor_records'), ('rain_adjustment_runs'),
      ('ecmwf_temperature_canary_runs')) relation(name)
    JOIN pg_attribute attribute
      ON attribute.attrelid = to_regclass('public.' || relation.name)
    WHERE attribute.attname IN ('stage_receipt_sha256', 'archive_commit_ordinal',
      'predecessor_frontier_sha256', 'revision_frontier_sha256',
      'revision_receipt_sha256', 'archive_committed_at',
      'adjustment_revision_receipt') AND NOT attribute.attisdropped
  LOOP
    EXECUTE format('REVOKE ALL (%I) ON %I FROM PUBLIC, weather_api, weather_ingest, weather_training_export',
      revision_column.attname, revision_column.relation_name);
  END LOOP;
END;
$adjustment_revision_frontier_acl$;

-- reconcile the bounded rolling-registration annex with function-only access
DO $adjustment_rolling_registration_acl$
DECLARE
  object_count integer;
  exact_count integer;
  registration_column record;
BEGIN
  SELECT count(*) INTO object_count FROM (
    SELECT relation.oid FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public' AND relation.relname IN (
      'adjustment_registration_schedule_v3',
      'adjustment_registration_horizons_v3',
      'adjustment_shadow_registration_windows_v3'
    )
    UNION ALL
    SELECT procedure.oid FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public' AND procedure.proname IN (
      'weather_guard_adjustment_registration_v3',
      'weather_reject_adjustment_registration_v3_mutation',
      'weather_initialize_adjustment_registration_schedule_v3',
      'weather_register_adjustment_shadow_v3',
      'adjustment_shadow_registration_slot_v3'
    )
  ) objects;
  -- preserve older complete schemas only when the entire annex is absent
  IF object_count = 0 THEN
    IF EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0021_adjustment_rolling_registration.sql') THEN
      RAISE EXCEPTION 'adjustment rolling registration schema is partially absent';
    END IF;
    RETURN;
  END IF;
  SELECT count(*) INTO exact_count FROM (VALUES
    (to_regclass('public.adjustment_registration_schedule_v3')::oid),
    (to_regclass('public.adjustment_registration_horizons_v3')::oid),
    (to_regclass('public.adjustment_shadow_registration_windows_v3')::oid),
    (to_regprocedure('public.weather_guard_adjustment_registration_v3()')::oid),
    (to_regprocedure('public.weather_reject_adjustment_registration_v3_mutation()')::oid),
    (to_regprocedure('public.weather_initialize_adjustment_registration_schedule_v3(jsonb)')::oid),
    (to_regprocedure('public.weather_register_adjustment_shadow_v3(jsonb)')::oid),
    (to_regprocedure('public.adjustment_shadow_registration_slot_v3(text)')::oid)
  ) expected(oid) WHERE oid IS NOT NULL;
  -- reject partial, overloaded, or unledgered object graphs
  IF object_count <> 8 OR exact_count <> 8
    OR NOT EXISTS (SELECT 1 FROM schema_migrations
      WHERE name = '0021_adjustment_rolling_registration.sql') THEN
    RAISE EXCEPTION 'adjustment rolling registration schema is incomplete';
  END IF;
  -- bind every table and function to the database owner
  IF EXISTS (
    SELECT 1 FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public' AND relation.relname IN (
      'adjustment_registration_schedule_v3',
      'adjustment_registration_horizons_v3',
      'adjustment_shadow_registration_windows_v3'
    ) AND relation.relowner <> 'weather_owner'::regrole
  ) OR EXISTS (
    SELECT 1 FROM pg_proc procedure
    JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
    WHERE namespace.nspname = 'public' AND procedure.proname IN (
      'weather_guard_adjustment_registration_v3',
      'weather_reject_adjustment_registration_v3_mutation',
      'weather_initialize_adjustment_registration_schedule_v3',
      'weather_register_adjustment_shadow_v3',
      'adjustment_shadow_registration_slot_v3'
    ) AND procedure.proowner <> 'weather_owner'::regrole
  ) THEN
    RAISE EXCEPTION 'adjustment rolling registration ownership is invalid';
  END IF;
  -- require exact volatility, definer, return and search-path boundaries
  IF EXISTS (
    SELECT 1 FROM (VALUES
      ('weather_guard_adjustment_registration_v3',
        'public.weather_guard_adjustment_registration_v3()', 'trigger', 'v', false),
      ('weather_reject_adjustment_registration_v3_mutation',
        'public.weather_reject_adjustment_registration_v3_mutation()', 'trigger', 'v', false),
      ('weather_initialize_adjustment_registration_schedule_v3',
        'public.weather_initialize_adjustment_registration_schedule_v3(jsonb)', 'jsonb', 'v', true),
      ('weather_register_adjustment_shadow_v3',
        'public.weather_register_adjustment_shadow_v3(jsonb)', 'jsonb', 'v', true),
      ('adjustment_shadow_registration_slot_v3',
        'public.adjustment_shadow_registration_slot_v3(text)', 'jsonb', 's', true)
    ) expected(name, identity, return_type, volatility, security_definer)
    JOIN pg_proc procedure ON procedure.oid = to_regprocedure(expected.identity)
    WHERE procedure.prorettype::regtype::text <> expected.return_type
      OR procedure.provolatile <> expected.volatility::"char"
      OR procedure.prosecdef IS DISTINCT FROM expected.security_definer
      OR procedure.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, public']
  ) THEN
    RAISE EXCEPTION 'adjustment rolling registration function hardening differs';
  END IF;
  -- require every append-only trigger to remain ordinary and enabled
  IF (SELECT count(*) FROM pg_trigger trigger
      JOIN pg_class relation ON relation.oid = trigger.tgrelid
      JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
      WHERE namespace.nspname = 'public' AND relation.relname IN (
        'adjustment_registration_schedule_v3',
        'adjustment_registration_horizons_v3',
        'adjustment_shadow_registration_windows_v3'
      ) AND NOT trigger.tgisinternal AND trigger.tgenabled = 'O'
        AND trigger.tgname IN (
          'adjustment_registration_schedule_v3_guard_insert',
          'adjustment_registration_schedule_v3_guard_mutation',
          'adjustment_registration_schedule_v3_guard_truncate',
          'adjustment_registration_horizons_v3_guard_insert',
          'adjustment_registration_horizons_v3_guard_mutation',
          'adjustment_registration_horizons_v3_guard_truncate',
          'adjustment_shadow_registration_windows_v3_guard_insert',
          'adjustment_shadow_registration_windows_v3_guard_mutation',
          'adjustment_shadow_registration_windows_v3_guard_truncate'
        ) AND trigger.tgfoid = CASE
          WHEN trigger.tgname LIKE '%_guard_insert'
            THEN to_regprocedure('public.weather_guard_adjustment_registration_v3()')
          ELSE to_regprocedure('public.weather_reject_adjustment_registration_v3_mutation()')
        END AND trigger.tgtype = CASE
          WHEN trigger.tgname LIKE '%_guard_insert' THEN 7
          WHEN trigger.tgname LIKE '%_guard_mutation' THEN 27
          ELSE 34
        END) <> 9 THEN
    RAISE EXCEPTION 'adjustment rolling registration trigger graph is incomplete';
  END IF;

  REVOKE ALL ON adjustment_registration_schedule_v3,
    adjustment_registration_horizons_v3,
    adjustment_shadow_registration_windows_v3
  FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  REVOKE EXECUTE ON FUNCTION weather_guard_adjustment_registration_v3(),
    weather_reject_adjustment_registration_v3_mutation(),
    weather_initialize_adjustment_registration_schedule_v3(jsonb),
    weather_register_adjustment_shadow_v3(jsonb),
    adjustment_shadow_registration_slot_v3(text)
  FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
  -- erase every historical direct column privilege
  FOR registration_column IN
    SELECT relation.relname AS table_name, attribute.attname AS column_name
    FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    JOIN pg_attribute attribute ON attribute.attrelid = relation.oid
    WHERE namespace.nspname = 'public' AND relation.relname IN (
      'adjustment_registration_schedule_v3',
      'adjustment_registration_horizons_v3',
      'adjustment_shadow_registration_windows_v3'
    ) AND attribute.attnum > 0 AND NOT attribute.attisdropped
  LOOP
    EXECUTE format(
      'REVOKE ALL (%I) ON %I FROM PUBLIC, weather_api, weather_ingest, weather_training_export',
      registration_column.column_name,
      registration_column.table_name
    );
  END LOOP;

  GRANT EXECUTE ON FUNCTION weather_register_adjustment_shadow_v3(jsonb),
    adjustment_shadow_registration_slot_v3(text)
  TO weather_api, weather_ingest;
  -- expose only the bounded value-free schedule status to the export role
  GRANT EXECUTE ON FUNCTION adjustment_shadow_registration_slot_v3(text)
  TO weather_training_export;
END;
$adjustment_rolling_registration_acl$;

ALTER DEFAULT PRIVILEGES FOR ROLE weather_owner IN SCHEMA public REVOKE ALL ON TABLES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE weather_owner IN SCHEMA public REVOKE ALL ON SEQUENCES FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE weather_owner IN SCHEMA public REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
