-- A session's chain, in order.
SELECT id, seq, type, payload, prev_hash, hash, at FROM events WHERE session_id = $1 ORDER BY seq
