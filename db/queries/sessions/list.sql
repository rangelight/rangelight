-- The newest sessions first.
SELECT id, tracker, ticket_id, ticket_key, title, ticket_url, namespace, repo, state, phase,
  generation, created_at
FROM sessions
ORDER BY created_at DESC, id DESC
LIMIT $1
