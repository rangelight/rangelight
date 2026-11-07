-- The session's last event, to chain the next one to. The caller holds the session's row lock.
SELECT seq, hash FROM events WHERE session_id = $1 ORDER BY seq DESC LIMIT 1
