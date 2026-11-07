-- The newest sessions first.
SELECT id, tracker, ticket_id, ticket_key, title, namespace, state, generation
FROM sessions
ORDER BY created_at DESC, id DESC
LIMIT $1
