-- What the runner needs to run a session, and what it reports back (RAN-35). Forward-only, expand:
-- rows from before keep empty values, which the runner refuses (the ticket rule needs criteria).

-- The ticket's brief and criteria (api/jsonschema/ticket.v1alpha1.schema.json), and the workspace.
ALTER TABLE sessions
  ADD COLUMN description text NOT NULL DEFAULT '',
  ADD COLUMN criteria text[] NOT NULL DEFAULT '{}',
  ADD COLUMN repo text NOT NULL DEFAULT '',
  ADD COLUMN base_ref text NOT NULL DEFAULT '',
  ADD COLUMN member text NOT NULL DEFAULT '',
  -- As the runner last reported it; NULL before the first report.
  ADD COLUMN phase text CHECK (
    phase IN ('queued', 'starting', 'running', 'hibernating', 'hibernated', 'waking', 'terminating', 'gone', 'failed')
  ),
  -- The message_id (a UUIDv7) of the report that set it: a later report sets the phase only if its
  -- id is greater, which is later in time, so a late delivery never undoes a newer phase.
  ADD COLUMN phase_message_id uuid;

-- New sessions must say where they work.
ALTER TABLE sessions
  ALTER COLUMN repo DROP DEFAULT,
  ALTER COLUMN base_ref DROP DEFAULT,
  ALTER COLUMN member DROP DEFAULT;

-- The runner message an event records (a SessionStatus's Envelope.message_id): one event per
-- message, however often it is delivered.
ALTER TABLE events ADD COLUMN message_id uuid UNIQUE;
