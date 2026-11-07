-- A new session, unless its ticket already has one: then no row comes back.
INSERT INTO sessions (id, tracker, ticket_id, ticket_key, title, namespace, state, generation, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, $8)
ON CONFLICT (tracker, ticket_id) DO NOTHING
RETURNING id
