-- The session whose lane runs in a namespace: who a lane's token is (the NATS auth callout).
SELECT id, tracker, ticket_id, ticket_key, title, ticket_url, namespace, repo, state, phase,
  pull_request_url, generation, created_at
FROM sessions
WHERE namespace = $1
