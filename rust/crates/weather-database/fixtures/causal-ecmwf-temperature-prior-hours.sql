
      SELECT DISTINCT ON (hour.valid_at)
        run.run_initialized_at AS "runInitializedAt",
        hour.valid_at AS "validAt",
        hour.model_lead_hours AS "modelLeadHours",
        hour.raw_temperature_c AS "rawTemperatureC",
        hour.raw_relative_humidity_percent AS "rawRelativeHumidityPercent",
        hour.raw_wind_speed_mps AS "rawWindSpeedMps",
        hour.content_hash AS "contentHash"
      FROM ecmwf_temperature_canary_hours hour
      JOIN ecmwf_temperature_canary_runs run ON run.id = hour.run_id
      JOIN sites site ON site.id = run.site_id
      WHERE site.slug = $1
        AND site.active
        AND hour.valid_at >= $2
        AND hour.valid_at <= $3
        AND hour.model_lead_hours BETWEEN 7 AND 18
        AND run.adapter_version = 'open-meteo-ecmwf-single-run/v1'
        AND run.run_initialized_at < $4
        AND run.first_received_at <= $4
      ORDER BY hour.valid_at ASC, run.run_initialized_at DESC, run.id DESC
    
