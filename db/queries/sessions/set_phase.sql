-- The session's phase from a report, unless a later report (a greater UUIDv7 message_id: uuid
-- compares bytewise, which is time order for v7) already set it; and its first pull request ($6,
-- NULL for none). The generation always moves on.
UPDATE sessions SET
  phase = CASE WHEN phase_message_id IS NULL OR $5 > phase_message_id THEN $2 ELSE phase END,
  phase_message_id = GREATEST(phase_message_id, $5),
  pull_request_url = COALESCE(pull_request_url, $6),
  generation = $3,
  updated_at = $4
WHERE id = $1
