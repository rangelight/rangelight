-- Test fixture: take every session's row lock, as a long command would.
SELECT id FROM sessions FOR UPDATE;
