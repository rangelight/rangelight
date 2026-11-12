-- Whether the session already has an event for this message (a runner's or its agent's).
SELECT EXISTS (SELECT 1 FROM events WHERE session_id = $1 AND message_id = $2) AS "recorded!"
