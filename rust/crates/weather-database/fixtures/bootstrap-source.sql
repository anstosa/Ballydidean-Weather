
            INSERT INTO sources (
              station_id,
              provider_id,
              source_key,
              source_kind,
              material_provider_config,
              source_config_fingerprint,
              capabilities,
              cadence_seconds,
              active
            )
            VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7::jsonb, $8, $9)
            ON CONFLICT (station_id, source_key) DO UPDATE SET
              cadence_seconds = EXCLUDED.cadence_seconds,
              active = EXCLUDED.active
            WHERE sources.source_config_fingerprint = EXCLUDED.source_config_fingerprint
            RETURNING id
