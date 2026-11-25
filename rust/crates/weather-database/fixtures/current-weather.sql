
      SELECT
        
    wr.id,
    wr.content_hash AS "contentHash",
    wr.source_id AS "sourceId",
    NULL::text AS "sourceConfigFingerprint",
    NULL::text AS "adapterVersion",
    NULL::text AS "contractEpoch",
    s.source_key AS "sourceKey",
    wr.source_kind AS "sourceKind",
    wr.valid_at AS "validAt",
    wr.product_run_at AS "productRunAt",
    wr.first_received_at AS "firstReceivedAt",
    wr.last_received_at AS "lastReceivedAt",
    wr.upstream_timezone AS "upstreamTimezone",
    wr.upstream_model AS "upstreamModel",
    wr.device_vendor AS "deviceVendor",
    wr.device_model AS "deviceModel",
    wr.device_serial AS "deviceSerial",
    wr.quality_metadata AS "qualityMetadata",
    wr.provider_metadata AS "providerMetadata",
    wr.revision_count AS "revisionCount",
    wr.temperature_c AS "temperatureC",
    wr.apparent_temperature_c AS "apparentTemperatureC",
    wr.precipitation_mm AS "precipitationMm",
    wr.wind_speed_mps AS "windSpeedMps",
    wr.wind_gust_mps AS "windGustMps",
    wr.pressure_hpa AS "pressureHpa",
    wr.relative_humidity_percent AS "relativeHumidityPercent",
    wr.cloud_cover_percent AS "cloudCoverPercent",
    wr.wind_direction_degrees AS "windDirectionDegrees",
    wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
    wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
    wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
    wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
    wr.soil_moisture_percent AS "soilMoisturePercent",
    wr.solar_radiation_wm2 AS "solarRadiationWm2",
    wr.uv_index AS "uvIndex",
    wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
    wr.water_level_m AS "waterLevelM",
    st.slug AS "stationSlug",
    si.slug AS "siteSlug",
    p.provider_key AS "providerKey"
  ,
        -- require complete three-hour evidence
        CASE
          WHEN wr.pressure_hpa IS NULL
            OR pressure_before.pressure_hpa IS NULL
            OR pressure_after.pressure_hpa IS NULL
          THEN NULL
          WHEN pressure_before.valid_at = pressure_after.valid_at
          THEN wr.pressure_hpa - pressure_before.pressure_hpa
          ELSE wr.pressure_hpa - (
            pressure_before.pressure_hpa
            + (pressure_after.pressure_hpa - pressure_before.pressure_hpa)
              * EXTRACT(EPOCH FROM (wr.valid_at - interval '3 hours' - pressure_before.valid_at))
              / EXTRACT(EPOCH FROM (pressure_after.valid_at - pressure_before.valid_at))
          )
        END AS "pressureChange3hHpa",
        pm25_context.hourly_concentrations AS "pm25HourlyConcentrations",
        pm25_context.hourly_by_sensor AS "pm25HourlyBySensor",
        pm25_context.through_start AS "pm25HourlyThrough"
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT 
    candidate.id,
    candidate.source_id,
    candidate.source_kind,
    candidate.valid_at,
    candidate.product_run_at,
    candidate.first_received_at,
    candidate.last_received_at,
    candidate.upstream_timezone,
    candidate.upstream_model,
    candidate.device_vendor,
    candidate.device_model,
    candidate.device_serial,
    candidate.quality_metadata,
    candidate.provider_metadata,
    candidate.temperature_c,
    candidate.apparent_temperature_c,
    candidate.precipitation_mm,
    candidate.wind_speed_mps,
    candidate.wind_gust_mps,
    candidate.pressure_hpa,
    candidate.relative_humidity_percent,
    candidate.cloud_cover_percent,
    candidate.wind_direction_degrees,
    candidate.content_hash,
    candidate.revision_count,
    candidate.black_globe_temperature_c,
    candidate.pm25_micrograms_per_cubic_meter,
    candidate.precipitation_rate_mm_per_hour,
    candidate.soil_electrical_conductivity_us_cm,
    candidate.soil_moisture_percent,
    candidate.solar_radiation_wm2,
    candidate.uv_index,
    candidate.wet_bulb_globe_temperature_c,
    candidate.water_level_m
  
        FROM weather_records candidate
        WHERE candidate.source_id = s.id
          AND candidate.valid_at <= $4::timestamptz
        ORDER BY candidate.valid_at DESC, candidate.id DESC
        LIMIT 1
      ) wr ON true
      -- find pressure immediately before target
      LEFT JOIN LATERAL (
        SELECT candidate.valid_at, candidate.pressure_hpa
        FROM weather_records candidate
        WHERE candidate.source_id = wr.source_id
          AND candidate.pressure_hpa IS NOT NULL
          AND candidate.valid_at <= wr.valid_at - interval '3 hours'
          AND candidate.valid_at >= wr.valid_at - interval '3 hours 30 minutes'
        ORDER BY candidate.valid_at DESC, candidate.id DESC
        LIMIT 1
      ) pressure_before ON true
      -- find pressure immediately after target
      LEFT JOIN LATERAL (
        SELECT candidate.valid_at, candidate.pressure_hpa
        FROM weather_records candidate
        WHERE candidate.source_id = wr.source_id
          AND candidate.pressure_hpa IS NOT NULL
          AND candidate.valid_at >= wr.valid_at - interval '3 hours'
          AND candidate.valid_at <= wr.valid_at - interval '2 hours 30 minutes'
        ORDER BY candidate.valid_at ASC, candidate.id ASC
        LIMIT 1
      ) pressure_after ON true
      -- build source-local hourly particulate means for the EPA NowCast input
      LEFT JOIN LATERAL (
        -- keep timestamp selectivity visible and avoid expensive per-request JIT
        WITH bounds AS NOT MATERIALIZED (
          SELECT date_trunc('hour', $4::timestamptz) - interval '1 hour' AS through_start
        ),
        slots AS (
          SELECT
            age,
            bounds.through_start - age * interval '1 hour' AS hour_start
          FROM bounds
          CROSS JOIN generate_series(0, 11) AS age
        ),
        raw_pm25 AS MATERIALIZED (
          SELECT
            date_trunc('hour', candidate.valid_at) AS hour_start,
            candidate.pm25_micrograms_per_cubic_meter,
            candidate.provider_metadata
          FROM weather_records candidate
          CROSS JOIN bounds
          WHERE candidate.source_id = s.id
            AND s.source_kind = 'physical_sensor'
            AND candidate.source_kind = 'physical_sensor'
            AND (
              candidate.pm25_micrograms_per_cubic_meter IS NOT NULL
              OR COALESCE(
                jsonb_path_exists(
                  candidate.provider_metadata,
                  '$.property_sensors[*].readings.pm25MicrogramsPerCubicMeter'
                ),
                false
              )
            )
            AND candidate.valid_at >= bounds.through_start - interval '11 hours'
            AND candidate.valid_at < bounds.through_start + interval '1 hour'
        ),
        main_hourly AS (
          SELECT
            hour_start,
            AVG(pm25_micrograms_per_cubic_meter) AS concentration
          FROM raw_pm25
          WHERE pm25_micrograms_per_cubic_meter >= 0
            AND pm25_micrograms_per_cubic_meter < 'Infinity'::double precision
          GROUP BY hour_start
        ),
        sensor_hourly AS (
          SELECT
            sensor.value ->> 'key' AS sensor_key,
            raw_pm25.hour_start,
            AVG((sensor.value #>> '{readings,pm25MicrogramsPerCubicMeter}')::double precision) AS concentration
          FROM raw_pm25
          CROSS JOIN LATERAL jsonb_array_elements(
            CASE
              WHEN jsonb_typeof(raw_pm25.provider_metadata -> 'property_sensors') = 'array'
              THEN raw_pm25.provider_metadata -> 'property_sensors'
              ELSE '[]'::jsonb
            END
          ) AS sensor(value)
          WHERE sensor.value ->> 'model' = 'WH41'
            AND sensor.value ->> 'key' ~ '^air-quality-[1-9][0-9]?$'
            AND jsonb_typeof(sensor.value #> '{readings,pm25MicrogramsPerCubicMeter}') = 'number'
            AND (sensor.value #>> '{readings,pm25MicrogramsPerCubicMeter}')::double precision >= 0
            AND (sensor.value #>> '{readings,pm25MicrogramsPerCubicMeter}')::double precision < 'Infinity'::double precision
          GROUP BY sensor.value ->> 'key', raw_pm25.hour_start
        ),
        sensor_contexts AS (
          SELECT
            sensor_keys.sensor_key,
            jsonb_agg(sensor_hourly.concentration ORDER BY slots.age) AS hourly_concentrations
          FROM (SELECT DISTINCT sensor_key FROM sensor_hourly) sensor_keys
          CROSS JOIN slots
          LEFT JOIN sensor_hourly
            ON sensor_hourly.sensor_key = sensor_keys.sensor_key
            AND sensor_hourly.hour_start = slots.hour_start
          GROUP BY sensor_keys.sensor_key
        )
        SELECT
          CASE
            WHEN COUNT(main_hourly.concentration) = 0 THEN NULL
            ELSE ARRAY_AGG(main_hourly.concentration ORDER BY slots.age)
          END AS hourly_concentrations,
          (SELECT jsonb_object_agg(sensor_key, hourly_concentrations) FROM sensor_contexts) AS hourly_by_sensor,
          bounds.through_start
        FROM bounds
        CROSS JOIN slots
        LEFT JOIN main_hourly ON main_hourly.hour_start = slots.hour_start
        GROUP BY bounds.through_start
      ) pm25_context ON true
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind IN ('physical_sensor', 'model_current', 'reanalysis')
        AND weather_source_is_current(s.id)
        AND ($2::text IS NULL OR st.slug = $2)
        AND ($3::bigint IS NULL OR s.id = $3)
      ORDER BY wr.source_id
    