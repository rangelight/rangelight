-- Test fixture: rewrite history, which the events trigger refuses.
UPDATE events SET payload = '\x00';
