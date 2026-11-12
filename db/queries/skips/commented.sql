-- The skip of ticket ($1, $2) at version $3 was commented on at $4. A no-op if the ticket was edited
-- since: the new version is commented on.
UPDATE ticket_skips SET commented_at = $4
WHERE tracker = $1 AND ticket_id = $2 AND version = $3
