
      SELECT run.run_initialized_at AS "runInitializedAt"
      FROM ecmwf_temperature_canary_runs run
      JOIN sites site ON site.id = run.site_id
      WHERE site.slug = $1
        AND site.active
        AND run.run_initialized_at >= $2
        AND run.run_initialized_at <= $3
      ORDER BY run.run_initialized_at DESC
    