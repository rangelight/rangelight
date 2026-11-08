-- Test fixture: a session whose events an old relay can't decode (say, a Change variant from a
-- newer release), with two unsent outbox rows.
INSERT INTO sessions (id, tracker, ticket_id, ticket_key, title, namespace, repo, base_ref, member, state, generation, created_at, updated_at)
VALUES ('00000000-0000-0000-0000-0000000000a1', 'linear', 'poison', 'POISON-1', 'p', 'rl-poison-1', 'shop', 'main', 'ariel', 'hot', 2, now(), now());
INSERT INTO events (id, session_id, seq, type, payload, prev_hash, hash, at) VALUES
  ('00000000-0000-0000-0000-0000000000e1', '00000000-0000-0000-0000-0000000000a1', 1, 'session.future', '\xffff',
   decode(repeat('00', 32), 'hex'), decode(repeat('00', 32), 'hex'), now()),
  ('00000000-0000-0000-0000-0000000000e2', '00000000-0000-0000-0000-0000000000a1', 2, 'session.future', '\xffff',
   decode(repeat('00', 32), 'hex'), decode(repeat('00', 32), 'hex'), now());
INSERT INTO outbox (event_id) VALUES ('00000000-0000-0000-0000-0000000000e1'), ('00000000-0000-0000-0000-0000000000e2');
