
          INSERT INTO providers (
            provider_key,
            display_name,
            attribution_label,
            attribution_url,
            active
          )
          VALUES ($1, $2, $3, $4, $5)
          ON CONFLICT (provider_key) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            attribution_label = EXCLUDED.attribution_label,
            attribution_url = EXCLUDED.attribution_url,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
