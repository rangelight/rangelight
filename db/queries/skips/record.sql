-- Intake skipped ticket ($1, $2) at version $3, saying $4. A new version is a new skip to comment on;
-- the same one changes nothing.
INSERT INTO ticket_skips (tracker, ticket_id, version, body) VALUES ($1, $2, $3, $4)
ON CONFLICT (tracker, ticket_id) DO UPDATE SET
  version = EXCLUDED.version, body = EXCLUDED.body,
  commented_at = NULL, tried_at = NULL, refused_at = NULL
WHERE ticket_skips.version <> EXCLUDED.version
