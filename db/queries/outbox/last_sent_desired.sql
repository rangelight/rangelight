-- Each session's last published desired state (its newest sent created or state_changed event),
-- with what the relay needs to publish it again: what it puts back into an empty desired stream.
SELECT DISTINCT ON (e.session_id) o.id AS outbox_id, e.id AS event_id, e.session_id, e.seq,
  e.payload, e.at, s.namespace, s.ticket_key, s.title, s.description, s.criteria, s.repo,
  s.base_ref, s.member
FROM outbox o
JOIN events e ON e.id = o.event_id
JOIN sessions s ON s.id = e.session_id
WHERE o.sent_at IS NOT NULL
  AND e.type IN ('session.created', 'session.state_changed')
ORDER BY e.session_id, e.seq DESC
