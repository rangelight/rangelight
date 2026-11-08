-- A new session, unless its ticket already has one: then no row comes back.
INSERT INTO sessions (
  id, tracker, ticket_id, ticket_key, title, description, criteria, ticket_url, namespace, repo,
  base_ref, member, state, generation, created_at, updated_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 1, $14, $14)
ON CONFLICT (tracker, ticket_id) DO NOTHING
RETURNING id
