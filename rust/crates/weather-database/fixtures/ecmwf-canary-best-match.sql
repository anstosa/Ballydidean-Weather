
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
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT candidate.product_run_at
        FROM weather_records candidate
        WHERE candidate.source_id = s.id
          AND candidate.source_kind = 'forecast'
          AND candidate.product_run_at IS NOT NULL
          AND candidate.valid_at >= $2
          AND candidate.valid_at < $3
          AND candidate.first_received_at <= $4
          AND candidate.last_received_at <= $4
          AND candidate.upstream_model = 'best_match'
          AND candidate.provider_metadata ->> 'dataset' = 'best_match'
        GROUP BY candidate.product_run_at
        HAVING COUNT(*) = 12
          AND MIN(candidate.valid_at) = $2
          AND MAX(candidate.valid_at) = $3::timestamptz - interval '1 hour'
          AND BOOL_AND(candidate.valid_at = date_trunc('hour', candidate.valid_at))
        ORDER BY candidate.product_run_at DESC
        LIMIT 1
      ) selected ON true
      JOIN weather_records wr ON wr.source_id = s.id
        AND wr.source_kind = 'forecast'
        AND wr.product_run_at = selected.product_run_at
        AND wr.valid_at >= $2
        AND wr.valid_at < $3
        AND wr.first_received_at <= $4
        AND wr.last_received_at <= $4
        AND wr.upstream_model = 'best_match'
        AND wr.provider_metadata ->> 'dataset' = 'best_match'
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind = 'forecast'
        AND s.source_key = 'open-meteo-forecast-v4'
        AND s.capabilities @> '["forecast"]'::jsonb
        AND weather_source_is_current(s.id)
      ORDER BY wr.valid_at ASC, wr.id ASC
      LIMIT 12
    
