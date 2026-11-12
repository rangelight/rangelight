-- The session's state, repo and pull request, locked until the command's transaction ends.
SELECT state, repo, pull_request_url FROM sessions WHERE id = $1 FOR UPDATE
