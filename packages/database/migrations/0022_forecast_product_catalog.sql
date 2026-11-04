SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '120s';

LOCK TABLE public.weather_records IN SHARE ROW EXCLUSIVE MODE;

CREATE TABLE public.forecast_products (
  source_id bigint NOT NULL REFERENCES public.sources(id) ON DELETE RESTRICT,
  product_run_at timestamptz NOT NULL,
  PRIMARY KEY (source_id, product_run_at)
);

ALTER TABLE public.forecast_products OWNER TO weather_owner;

-- add inserted product identities
CREATE FUNCTION public.weather_sync_forecast_products_after_insert()
RETURNS trigger
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
BEGIN
  INSERT INTO public.forecast_products (source_id, product_run_at)
  SELECT DISTINCT inserted.source_id, inserted.product_run_at
  FROM inserted_weather_records inserted
  WHERE inserted.source_kind = 'forecast'
  ON CONFLICT DO NOTHING;

  RETURN NULL;
END;
$function$;

-- remove unreferenced product identities
CREATE FUNCTION public.weather_sync_forecast_products_after_delete()
RETURNS trigger
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
BEGIN
  DELETE FROM public.forecast_products product
  USING (
    SELECT DISTINCT deleted.source_id, deleted.product_run_at
    FROM deleted_weather_records deleted
    WHERE deleted.source_kind = 'forecast'
  ) affected
  WHERE product.source_id = affected.source_id
    AND product.product_run_at = affected.product_run_at
    AND NOT EXISTS (
      SELECT 1
      FROM public.weather_records retained
      WHERE retained.source_id = affected.source_id
        AND retained.source_kind = 'forecast'
        AND retained.product_run_at = affected.product_run_at
    );

  RETURN NULL;
END;
$function$;

-- clear identities after raw truncation
CREATE FUNCTION public.weather_sync_forecast_products_after_truncate()
RETURNS trigger
LANGUAGE plpgsql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
BEGIN
  TRUNCATE TABLE public.forecast_products;
  RETURN NULL;
END;
$function$;

ALTER FUNCTION public.weather_sync_forecast_products_after_insert() OWNER TO weather_owner;
ALTER FUNCTION public.weather_sync_forecast_products_after_delete() OWNER TO weather_owner;
ALTER FUNCTION public.weather_sync_forecast_products_after_truncate() OWNER TO weather_owner;

REVOKE ALL ON FUNCTION public.weather_sync_forecast_products_after_insert()
FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
REVOKE ALL ON FUNCTION public.weather_sync_forecast_products_after_delete()
FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
REVOKE ALL ON FUNCTION public.weather_sync_forecast_products_after_truncate()
FROM PUBLIC, weather_api, weather_ingest, weather_training_export;

CREATE TRIGGER weather_records_forecast_products_insert
AFTER INSERT ON public.weather_records
REFERENCING NEW TABLE AS inserted_weather_records
FOR EACH STATEMENT
EXECUTE FUNCTION public.weather_sync_forecast_products_after_insert();

CREATE TRIGGER weather_records_forecast_products_delete
AFTER DELETE ON public.weather_records
REFERENCING OLD TABLE AS deleted_weather_records
FOR EACH STATEMENT
EXECUTE FUNCTION public.weather_sync_forecast_products_after_delete();

CREATE TRIGGER weather_records_forecast_products_truncate
AFTER TRUNCATE ON public.weather_records
FOR EACH STATEMENT
EXECUTE FUNCTION public.weather_sync_forecast_products_after_truncate();

INSERT INTO public.forecast_products (source_id, product_run_at)
SELECT DISTINCT raw.source_id, raw.product_run_at
FROM public.weather_records raw
WHERE raw.source_kind = 'forecast'
ON CONFLICT DO NOTHING;

-- reject any catalog drift
DO $block$
BEGIN
  IF EXISTS (
    SELECT DISTINCT raw.source_id, raw.product_run_at
    FROM public.weather_records raw
    WHERE raw.source_kind = 'forecast'
    EXCEPT
    SELECT product.source_id, product.product_run_at
    FROM public.forecast_products product
  ) OR EXISTS (
    SELECT product.source_id, product.product_run_at
    FROM public.forecast_products product
    EXCEPT
    SELECT DISTINCT raw.source_id, raw.product_run_at
    FROM public.weather_records raw
    WHERE raw.source_kind = 'forecast'
  ) THEN
    RAISE EXCEPTION 'forecast product catalog backfill is not exact';
  END IF;
END;
$block$;

REVOKE ALL ON TABLE public.forecast_products
FROM PUBLIC, weather_api, weather_ingest, weather_training_export;
GRANT SELECT ON TABLE public.forecast_products TO weather_api;

ANALYZE public.forecast_products;
