-- Test fixture: a session whose last published desired state the relay can't decode (say, a newer
-- release's change): a sent session.created row. Its id sorts before every UUIDv7, so a reseed meets
-- it first.
INSERT INTO sessions (id, tracker, ticket_id, ticket_key, title, ticket_url, namespace, repo, base_ref, member, state, generation, created_at, updated_at)
VALUES ('00000000-0000-0000-0000-0000000000a2', 'linear', 'poison-2', 'POISON-2', 'p', 'https://linear.app/example/issue/POISON-2', 'rl-poison-2', 'shop', 'main', 'ariel', 'hot', 1, now(), now());
INSERT INTO events (id, session_id, seq, type, payload, prev_hash, hash, at) VALUES
  ('00000000-0000-0000-0000-0000000000e3', '00000000-0000-0000-0000-0000000000a2', 1, 'session.created', '\xffff',
   decode(repeat('00', 32), 'hex'), decode(repeat('00', 32), 'hex'), now());
INSERT INTO outbox (event_id, sent_at) VALUES ('00000000-0000-0000-0000-0000000000e3', now());
