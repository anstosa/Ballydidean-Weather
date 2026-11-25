SELECT last_loop_at AS "lastLoopAt"
FROM worker_heartbeats
ORDER BY last_loop_at DESC, worker_instance ASC
LIMIT 1
