# relay

Publishes the outbox to NATS (ADR 0021, ADR 0031). Each round takes the oldest unsent events, in
order, and for each:

1. for an event that sets the desired state (created, state changed), publishes the session's
   whole `SessionDesired` (state, ticket, workspace, `generation` = the event's seq) in an
   `Envelope` to JetStream on `rl.<org>.runner.<runner>.desired.<session>`, with `Nats-Msg-Id` = the event
   id, and waits for the ack. A runner's own report (phase changed) isn't sent back to it;
2. publishes the `Event` on `rl.<org>.session.<id>.events` (core NATS) for `WatchSession`;
3. marks the outbox row sent.

A crash between 1 and 3 republishes on restart; a quick restart falls inside JetStream's 2-minute
duplicate window, and consumers dedupe on the event id and `generation` for anything later.

An event this relay can't decode (a newer release's change, mid rolling deploy) is set aside
(`outbox.failed_at`, logged as an error); only that session's later events wait, the rest keep
flowing. Other failures end the round, and rounds back off from 0.5s, doubling, up to 30s. The relay creates the stream `rl-<org>-desired`, or updates it to its
config: one message per subject and no age limit, so it holds each session's latest desired state
(a cold one is the tombstone), with `stream_replicas` replicas. At every start it puts back each session's last
published desired state from Postgres wherever the stream lost it (a wiped volume, the move into the `RL`
account when NATS sign-in came, a reseed cut short): each only onto its session's empty subject, never over
what's there (`Relay::reseed`). Then the outbox carries on with what's newer.

It signs in to NATS as the relay (ADR 0021 amendment, `rl-nats`): its projected token, `nats_token_file`, read
at every connect, and replies under `_INBOX_relay`. It may only publish desired state and session events and own
its stream.

Config: `configs/relay.default.toml`, then `/etc/rangelight/relay.toml`, then
`RANGELIGHT_RELAY_*`. `database_url` is a secret reference. `GET /healthz` on `listen` (8080) answers 200 once
Postgres, NATS and the stream are set up: the relay has started and is alive.

```sh
scripts/test/services.sh run cargo nextest run -p relay
docker build -f build/package/docker/relay/Dockerfile .
```

Code: `src/lib.rs`, `src/config.rs`, `src/main.rs`. Tests: `tests/relay.rs`.
