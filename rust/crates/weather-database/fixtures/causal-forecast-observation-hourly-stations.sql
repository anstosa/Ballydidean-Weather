
      SELECT
        wr.id,
        wr.valid_at AS "validAt",
        wr.quality_metadata AS "qualityMetadata",
        wr.temperature_c AS "temperatureC",
        wr.relative_humidity_percent AS "relativeHumidityPercent",
        wr.wind_speed_mps AS "windSpeedMps",
        wr.wind_gust_mps AS "windGustMps",
        wr.wind_direction_degrees AS "windDirectionDegrees",
        s.source_key AS "sourceKey",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.material_provider_config ->> 'contractVersion' AS "adapterContract",
        st.slug AS "stationSlug"
      FROM weather_records wr
      JOIN sources s ON s.id = wr.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      WHERE si.slug = $1
        AND wr.valid_at >= $2::timestamptz - interval '5 minutes'
        AND wr.valid_at < $3::timestamptz + interval '5 minutes'
        AND wr.first_received_at <= $4
        AND wr.last_received_at <= $4
        AND s.source_kind = 'physical_sensor'
        AND s.source_key = ANY($5::text[])
      ORDER BY wr.valid_at ASC, wr.id ASC
    
