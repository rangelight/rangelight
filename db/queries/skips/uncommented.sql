-- The skips still to comment on: neither commented nor refused, the least recently tried first
-- (never tried before all).
SELECT tracker, ticket_id, version, body
FROM ticket_skips
WHERE commented_at IS NULL AND refused_at IS NULL
ORDER BY tried_at NULLS FIRST, tracker, ticket_id
LIMIT $1
