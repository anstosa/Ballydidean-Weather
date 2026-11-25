SELECT
  run.id,
  run.run_initialized_at AS "runInitializedAt",
  run.first_received_at AS "firstReceivedAt",
  run.upstream_model AS "upstreamModel",
  run.adapter_version AS "adapterVersion",
  run.provider_response_sha256 AS "providerResponseSha256",
  run.model_cycle AS "modelCycle",
  run.recent_error_state AS "recentErrorState",
  run.recent_error_state_sha256 AS "recentErrorStateSha256",
  run.state_status AS "stateStatus",
  run.state_reason AS "stateReason",
  run.content_hash AS "contentHash"
FROM ecmwf_temperature_canary_runs run
JOIN sites site ON site.id = run.site_id
WHERE site.slug = $1
  AND site.active
  AND run.first_received_at <= $2
  AND run.run_initialized_at + interval '6 hours' <= $2
ORDER BY run.run_initialized_at DESC, run.first_received_at DESC, run.id DESC
LIMIT 1
