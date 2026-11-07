# relay

Publishes the outbox to NATS (ADR 0021, ADR 0031). Each round takes the oldest unsent events, in
order, and for each:

1. publishes the session's `SessionDesired` in an `Envelope` to JetStream on
   `rl.<org>.runner.<runner>.desired`, with `Nats-Msg-Id` = the event id, and waits for the ack;
2. publishes the `Event` on `rl.<org>.session.<id>.events` (core NATS) for `WatchSession`;
3. marks the outbox row sent.

A crash between 1 and 3 republishes on restart; a quick restart falls inside JetStream's 2-minute
duplicate window, and consumers dedupe on the event id and `generation` for anything later.

An event this relay can't decode (a newer release's change, mid rolling deploy) is set aside
(`outbox.failed_at`, logged as an error); only that session's later events wait, the rest keep
flowing. Other failures end the round, and rounds back off from 0.5s, doubling, up to 30s. The relay creates the stream `rl-<org>-desired` if it is
missing (limits retention, 7 days, `stream_replicas` replicas).

Config: `configs/relay.default.toml`, then `/etc/rangelight/relay.toml`, then
`RANGELIGHT_RELAY_*`. `database_url` is a secret reference.

```sh
scripts/test/services.sh run cargo nextest run -p relay
docker build -f build/package/docker/relay/Dockerfile .
```

Code: `src/lib.rs`, `src/config.rs`, `src/main.rs`. Tests: `tests/relay.rs`.
