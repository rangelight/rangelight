-- The oldest unsent events, with what the relay needs to publish them: the event, and its
-- session's ticket and workspace. A session with a failed row publishes nothing more, so its
-- events never go out of order.
SELECT o.id AS outbox_id, e.id AS event_id, e.session_id, e.seq, e.payload, e.at, s.namespace,
  s.ticket_key, s.title, s.description, s.criteria, s.ticket_url, s.repo, s.base_ref, s.member
FROM outbox o
JOIN events e ON e.id = o.event_id
JOIN sessions s ON s.id = e.session_id
WHERE o.sent_at IS NULL
  AND NOT EXISTS (
    SELECT 1 FROM outbox f JOIN events fe ON fe.id = f.event_id
    WHERE f.failed_at IS NOT NULL AND fe.session_id = e.session_id
  )
ORDER BY o.id
LIMIT $1
