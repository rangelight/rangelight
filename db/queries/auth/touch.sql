-- The member behind a login that is neither past its absolute end nor idle for $2 seconds; the
-- login counts as used now.
UPDATE logins SET last_seen_at = now()
WHERE token_hash = $1 AND expires_at > now() AND last_seen_at > now() - make_interval(secs => $2)
RETURNING member
