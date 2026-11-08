-- Every session's last seq: what a WatchSessions stream checks each change against.
SELECT id, generation FROM sessions
