-- Whether a runner message already has its event.
SELECT EXISTS (SELECT 1 FROM events WHERE message_id = $1) AS "recorded!"
