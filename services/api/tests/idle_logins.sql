-- Every login was last used just over 8 hours (the idle timeout) ago.
UPDATE logins SET last_seen_at = now() - interval '8 hours 1 minute';
