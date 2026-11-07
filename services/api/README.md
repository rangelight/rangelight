# api

The control plane's front door (ADR 0031, ADR 0033):

- **Linear intake.** Every 30 seconds it lists the configured team's open issues labelled
  `rangelight` and creates one session per issue. Session, first event and outbox row are written
  in one transaction; the `UNIQUE (tracker, ticket_id)` constraint makes a re-poll a no-op. An
  issue that can't be used, or a command that fails, is logged and skipped; the round goes on.
- **Session API**, Connect-RPC (`api/proto/rangelight/session/v1alpha1/session.proto`), on
  `listen` (8080): `ListSessions`, and the server stream `WatchSession`, fed by the relay's NATS
  fan-out (`rl.<org>.session.<id>.events`), never by polling Postgres. A seq gap (a slow
  consumer) ends the stream with `ABORTED`.
- It applies the migrations in `db/migrations/` at startup.

Config: `configs/api.default.toml`, then `/etc/rangelight/api.toml`, then `RANGELIGHT_API_*`.
`linear_team` has no default; `linear_url` must be https. Secrets are references: `database_url` and `linear_api_key` name
mounted Secret files (`secret://<name>#<key>` → `/var/run/rangelight/secrets/<name>/<key>`). The
Linear key is read at each poll and never logged.

Not here yet: authentication (OIDC, RAN-37) and authorisation (Cedar).

```sh
scripts/check.sh rust                                         # everything, with Postgres and NATS
scripts/test/services.sh run cargo nextest run -p api         # just these tests
docker build -f build/package/docker/api/Dockerfile .         # the image
```

Code: `src/lib.rs` (intake loop, server), `src/sessions.rs` (handlers), `src/config.rs`,
`src/main.rs` (wiring). Tests: `tests/api.rs`, end to end through a Linear stub, the relay and a
real Connect client.
