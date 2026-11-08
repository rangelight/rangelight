-- Every login has reached its absolute end, though used just now.
UPDATE logins SET expires_at = now(), last_seen_at = now();
