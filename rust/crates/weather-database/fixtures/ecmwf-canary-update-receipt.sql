
            UPDATE ecmwf_temperature_canary_runs
            SET last_received_at = GREATEST(last_received_at, $2)
            WHERE id = $1
          
