-- Sets a row aside that the relay can't publish; its session's later rows wait behind it.
UPDATE outbox SET failed_at = now() WHERE id = $1 AND sent_at IS NULL
