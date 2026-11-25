
    SELECT run_initialized_at, model_sha256, input_sha256, forecast_claim_id,
      first_received_at, decision_at, generated_at, hours FROM rain_adjustment_runs
    WHERE decision_at <= $1::timestamptz AND decision_at > $1::timestamptz - interval '12 hours'
      AND generated_at <= $1::timestamptz
    ORDER BY run_initialized_at DESC, generated_at DESC LIMIT 1
  
