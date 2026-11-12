-- Commenting on the skip of ticket ($1, $2) at version $3 failed at $4: tried then, and refused for
-- good when $5. A no-op if the ticket was edited since.
UPDATE ticket_skips SET
  tried_at = $4::timestamptz,
  refused_at = CASE WHEN $5::boolean THEN $4::timestamptz END
WHERE tracker = $1 AND ticket_id = $2 AND version = $3
