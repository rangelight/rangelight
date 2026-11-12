-- An agent event's effect: when $2, the session's question becomes $3 (NULL: none). The
-- generation always moves on.
UPDATE sessions SET
  question = CASE WHEN $2::boolean THEN $3::text ELSE question END,
  generation = $4,
  updated_at = $5
WHERE id = $1
