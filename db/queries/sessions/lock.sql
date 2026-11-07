-- The session's state, locked until the command's transaction ends.
SELECT state FROM sessions WHERE id = $1 FOR UPDATE
