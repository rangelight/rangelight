-- The sessions whose pull request is still to link: neither linked nor refused, the least recently
-- tried first (never tried before all).
SELECT id, tracker, ticket_id, ticket_key, title, ticket_url, namespace, repo, member, state, phase,
  pull_request_url, generation, created_at
FROM sessions
WHERE pull_request_url IS NOT NULL AND pull_request_linked_at IS NULL
  AND pull_request_link_refused_at IS NULL
ORDER BY pull_request_link_tried_at NULLS FIRST, created_at, id
LIMIT $1
