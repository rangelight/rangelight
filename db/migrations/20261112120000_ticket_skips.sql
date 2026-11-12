-- The labelled tickets intake skips, and the comment that tells each one's author why (RAN-64):
-- one per ticket and version, never one per poll. A ticket no longer listed as skipped (valid now,
-- closed or unlabelled) loses its row.
CREATE TABLE ticket_skips (
  tracker text NOT NULL,
  ticket_id text NOT NULL,
  -- A hash of what the ticket was skipped for (title, description, reason): only an edit to those
  -- is a new version.
  version text NOT NULL,
  -- The comment to post.
  body text NOT NULL,
  -- When the comment was posted for this version.
  commented_at timestamptz,
  -- The last attempt that failed: the next rounds try the least recently tried first.
  tried_at timestamptz,
  -- When the tracker refused the comment for good (say, the ticket is gone): never tried again.
  refused_at timestamptz,
  PRIMARY KEY (tracker, ticket_id)
);
-- What each comment round reads: the skips still to comment on.
CREATE INDEX ticket_skips_uncommented ON ticket_skips (tried_at NULLS FIRST)
  WHERE commented_at IS NULL AND refused_at IS NULL;
