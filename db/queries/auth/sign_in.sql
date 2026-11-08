-- A login for a member, binding the member to the account on its first sign-in. No row comes
-- back when another account already owns the member: the sign-in is refused. Logins past their
-- end, or idle for $6 seconds, go at the same moment.
WITH expired AS (
  DELETE FROM logins
  WHERE expires_at <= now() OR last_seen_at <= now() - make_interval(secs => $6)
), bound AS (
  INSERT INTO members (member, issuer, subject) VALUES ($2, $3, $4)
  ON CONFLICT (member) DO UPDATE SET member = excluded.member
  WHERE members.issuer = excluded.issuer AND members.subject = excluded.subject
  RETURNING member
)
INSERT INTO logins (token_hash, member, expires_at)
SELECT $1, member, now() + make_interval(secs => $5) FROM bound
RETURNING member
