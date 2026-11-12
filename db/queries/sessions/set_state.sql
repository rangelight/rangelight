-- The session's desired state. Only a hot session has a lane to wait on: any other state closes
-- its open question.
UPDATE sessions SET
  state = $2,
  question = CASE WHEN $2 = 'hot' THEN question END,
  generation = $3,
  updated_at = $4
WHERE id = $1
