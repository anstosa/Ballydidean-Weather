
              INSERT INTO ecmwf_temperature_canary_hours (
                run_id,
                valid_at,
                model_lead_hours,
                raw_temperature_c,
                raw_relative_humidity_percent,
                raw_wind_speed_mps,
                content_hash
              )
              VALUES ($1, $2, $3, $4, $5, $6, $7)
            
