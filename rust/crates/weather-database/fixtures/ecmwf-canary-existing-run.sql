
            SELECT 
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
  
            FROM ecmwf_temperature_canary_runs
            WHERE site_id = $1 AND run_initialized_at = $2
            FOR UPDATE
          
