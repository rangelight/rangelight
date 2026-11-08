-- One session.
SELECT id, tracker, ticket_id, ticket_key, title, ticket_url, namespace, repo, state, phase,
  generation, created_at
FROM sessions
WHERE id = $1
