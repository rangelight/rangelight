-- A link of the session's pull request $2 failed at $3: tried then, and refused for good when $4.
-- A no-op if its pull request isn't $2.
UPDATE sessions SET
  pull_request_link_tried_at = $3::timestamptz,
  pull_request_link_refused_at = CASE WHEN $4::boolean THEN $3::timestamptz END
WHERE id = $1 AND pull_request_url = $2
