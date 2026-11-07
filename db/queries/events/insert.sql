-- One event, linked by rl-chain; UNIQUE (session_id, seq) refuses a concurrent append, and
-- UNIQUE (message_id) a second event for one runner message.
INSERT INTO events (id, session_id, seq, type, payload, prev_hash, hash, at, message_id)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
