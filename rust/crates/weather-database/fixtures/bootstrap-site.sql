
          INSERT INTO sites (slug, display_name, latitude, longitude, timezone, active)
          VALUES ($1, $2, $3, $4, $5, $6)
          ON CONFLICT (slug) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            latitude = EXCLUDED.latitude,
            longitude = EXCLUDED.longitude,
            timezone = EXCLUDED.timezone,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
