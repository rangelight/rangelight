-- The session's ticket links its pull request $2 since $3. A no-op if its pull request isn't $2.
UPDATE sessions SET pull_request_linked_at = $3 WHERE id = $1 AND pull_request_url = $2
