-- The lane's pull request, as its runner first reported it, and its link on the ticket (RAN-56).
-- Forward-only, expand: all NULL until then. The api only writes PRs of the repo's upstream
-- (rl-domain's Upstream checks them exactly as this check does: scheme, host and /pull/ as written).
ALTER TABLE sessions
  ADD COLUMN pull_request_url text CHECK (pull_request_url LIKE 'https://github.com/%/pull/%'),
  -- When the ticket was linked to it.
  ADD COLUMN pull_request_linked_at timestamptz,
  -- The last link attempt that failed: the next rounds try the least recently tried first.
  ADD COLUMN pull_request_link_tried_at timestamptz,
  -- When the tracker refused the link for good (say, the ticket is gone): never tried again.
  ADD COLUMN pull_request_link_refused_at timestamptz;
-- What each link round reads: the sessions whose PR is still to link.
CREATE INDEX sessions_pull_request_unlinked
  ON sessions (pull_request_link_tried_at NULLS FIRST, created_at)
  WHERE pull_request_url IS NOT NULL AND pull_request_linked_at IS NULL
    AND pull_request_link_refused_at IS NULL;
