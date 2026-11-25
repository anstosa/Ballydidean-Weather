SELECT
  valid_at AS "validAt",
  model_lead_hours AS "modelLeadHours",
  raw_temperature_c AS "rawTemperatureC",
  raw_relative_humidity_percent AS "rawRelativeHumidityPercent",
  raw_wind_speed_mps AS "rawWindSpeedMps",
  content_hash AS "contentHash"
FROM ecmwf_temperature_canary_hours
WHERE run_id = $1
  AND model_lead_hours BETWEEN 7 AND 18
  AND valid_at >= $2
  AND valid_at < $3
ORDER BY valid_at ASC
