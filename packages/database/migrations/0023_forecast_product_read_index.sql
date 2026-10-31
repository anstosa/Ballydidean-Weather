SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '120s';

-- bound one forecast product before its time range
CREATE INDEX weather_records_forecast_product_valid_idx
ON public.weather_records (
  source_id,
  product_run_at DESC,
  valid_at ASC,
  id ASC
)
WHERE source_kind = 'forecast';
