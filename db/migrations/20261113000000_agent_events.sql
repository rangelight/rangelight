-- A lane's agent events (RAN-63). Forward-only, expand.
-- Waiting for you: the open question of the lane's last ask, NULL when it isn't waiting (and for
-- every session until its agent asks). The api holds it to the agent-event schema's 1 to 2000
-- characters.
ALTER TABLE sessions
  ADD COLUMN question text CHECK (char_length(question) BETWEEN 1 AND 2000);

-- A message id is unique per session, not across them: a lane chooses its events' ids, so one
-- must never decide another session's dedupe (a runner report's included).
ALTER TABLE events ADD CONSTRAINT events_session_message_id UNIQUE (session_id, message_id);
ALTER TABLE events DROP CONSTRAINT events_message_id_key;
