-- Sessions, their chained event log and the outbox (ADR 0029, ADR 0031). Forward-only.

-- Repositories Rangelight may work on. Registered by the repo onboarding (ADR 0047).
CREATE TABLE repos (
  id uuid PRIMARY KEY,
  full_name text NOT NULL UNIQUE CHECK (full_name ~ '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$'),
  created_at timestamptz NOT NULL DEFAULT now()
);

-- Current state, authoritative for "now".
CREATE TABLE sessions (
  id uuid PRIMARY KEY,
  repo_id uuid REFERENCES repos (id),
  tracker text NOT NULL CHECK (tracker IN ('linear')),
  ticket_id text NOT NULL,
  ticket_key text NOT NULL,
  title text NOT NULL,
  namespace text NOT NULL UNIQUE,
  state text NOT NULL CHECK (state IN ('hot', 'warm', 'cold')),
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  -- One ticket, one session: the constraint decides, not application code.
  UNIQUE (tracker, ticket_id)
);

-- The chained log, authoritative for "how we got here". Append-only.
CREATE TABLE events (
  id uuid PRIMARY KEY,
  session_id uuid NOT NULL REFERENCES sessions (id),
  seq bigint NOT NULL CHECK (seq > 0),
  type text NOT NULL,
  payload bytea NOT NULL,
  prev_hash bytea NOT NULL CHECK (length(prev_hash) = 32),
  hash bytea NOT NULL CHECK (length(hash) = 32),
  at timestamptz NOT NULL,
  UNIQUE (session_id, seq)
);

CREATE FUNCTION events_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'events are append-only';
END;
$$;

CREATE TRIGGER events_append_only BEFORE UPDATE OR DELETE OR TRUNCATE ON events
  FOR EACH STATEMENT EXECUTE FUNCTION events_append_only();

-- What the relay still has to publish, one row per event. `failed_at`: the relay couldn't
-- publish it (an undecodable payload); the session's later rows wait until someone looks.
-- ponytail: sent rows are kept for ever, one small row per event. Ceiling: table and index growth
-- at millions of events. Upgrade: a periodic DELETE of rows sent more than N days ago (the
-- events themselves stay), or partitioning by month.
CREATE TABLE outbox (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  event_id uuid NOT NULL UNIQUE REFERENCES events (id),
  sent_at timestamptz,
  failed_at timestamptz
);

CREATE INDEX outbox_unsent ON outbox (id) WHERE sent_at IS NULL;
CREATE INDEX outbox_failed ON outbox (event_id) WHERE failed_at IS NOT NULL;
