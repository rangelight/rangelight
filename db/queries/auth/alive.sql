-- Whether a login is neither past its absolute end nor idle for $2 seconds. Unlike touch.sql it
-- doesn't count as use: an open stream alone doesn't keep a login from going idle.
SELECT EXISTS (
  SELECT 1 FROM logins
  WHERE token_hash = $1 AND expires_at > now() AND last_seen_at > now() - make_interval(secs => $2)
) AS "alive!"
