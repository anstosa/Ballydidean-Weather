
          INSERT INTO ecmwf_temperature_canary_runs (
            site_id,
            run_initialized_at,
            first_received_at,
            last_received_at,
            upstream_model,
            adapter_version,
            provider_response_sha256,
            model_cycle,
            recent_error_state,
            recent_error_state_sha256,
            state_status,
            state_reason,
            content_hash
          )
          VALUES (
            $1, $2, $3, $3, $4, $5, $6, $7, $8::jsonb, $9, $10, $11, $12
          )
          ON CONFLICT (site_id, run_initialized_at) DO NOTHING
          RETURNING 
    id,
    run_initialized_at AS "runInitializedAt",
    first_received_at AS "firstReceivedAt",
    upstream_model AS "upstreamModel",
    adapter_version AS "adapterVersion",
    provider_response_sha256 AS "providerResponseSha256",
    model_cycle AS "modelCycle",
    recent_error_state AS "recentErrorState",
    recent_error_state_sha256 AS "recentErrorStateSha256",
    state_status AS "stateStatus",
    state_reason AS "stateReason",
    content_hash AS "contentHash"
  
        
