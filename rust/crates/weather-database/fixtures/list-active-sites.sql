
      SELECT
        si.slug AS "siteSlug",
        si.display_name AS "siteName",
        si.latitude,
        si.longitude,
        si.timezone,
        st.slug AS "stationSlug",
        st.display_name AS "stationName",
        st.station_kind AS "stationKind",
        st.latitude AS "stationLatitude",
        st.longitude AS "stationLongitude",
        s.id AS "sourceId",
        s.source_key AS "sourceKey",
        s.source_kind AS "sourceKind",
        p.provider_key AS "providerKey",
        p.display_name AS "providerName",
        p.attribution_label AS "attributionLabel",
        p.attribution_url AS "attributionUrl"
      FROM sites si
      JOIN stations st ON st.site_id = si.id AND st.active
      JOIN sources s ON s.station_id = st.id AND s.active
      JOIN providers p ON p.id = s.provider_id AND p.active
      WHERE si.active
        AND (
          s.source_kind <> 'forecast'
          OR s.capabilities @> '["forecast"]'::jsonb
        )
        AND weather_source_is_current(s.id)
      ORDER BY si.slug, st.slug, s.source_key
    