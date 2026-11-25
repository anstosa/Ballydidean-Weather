
            INSERT INTO stations (
              site_id,
              slug,
              display_name,
              station_kind,
              latitude,
              longitude,
              vendor,
              model,
              serial,
              active
            )
            VALUES ($1, $2, $3, 'physical', $4, $5, 'Ecowitt', $6, $7, $8)
            ON CONFLICT (site_id, slug) DO UPDATE SET
              display_name = EXCLUDED.display_name,
              station_kind = EXCLUDED.station_kind,
              latitude = EXCLUDED.latitude,
              longitude = EXCLUDED.longitude,
              vendor = EXCLUDED.vendor,
              model = EXCLUDED.model,
              serial = EXCLUDED.serial,
              active = EXCLUDED.active,
              updated_at = clock_timestamp()
            RETURNING id
