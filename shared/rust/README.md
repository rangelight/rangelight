# shared/rust

The Rust libraries, one workspace with the binaries in `services/` (ADR 0049 §3). Dependencies
point inward: binaries → adapters → application → domain.

| Crate | Holds |
|---|---|
| `domain` (`rl-domain`) | Session ids, the state machine (hot, warm, cold), tickets, members, commands, NATS subject names. Sync, no I/O. |
| `app` (`rl-app`) | Use cases (`intake`, `change_state`) and the seams they consume: `TicketSource`, `Clock`, `IdGen`. |
| `store` (`rl-store`) | The only Postgres access. One transaction per command: state, chained event (linked with `rl-chain`), outbox; and sign-ins, logins and members. SQL in `db/queries/`. Statement timeout 5s, lock timeout 2s. |
| `auth` (`rl-auth`) | OIDC sign-in, a trust boundary: authorization code with PKCE, ID token verified by `openidconnect` (signature via discovery, issuer, audience, expiry, nonce); the sign-in in progress sealed in its cookie (AES-256-GCM, HKDF of the client secret). |
| `chain` (`rl-chain`) | Canonical event payloads and the hash chain: `sha256(prev_hash ‖ event id ‖ at µs ‖ payload)`, verified with proptest. |
| `config` (`rl-config`) | The config loader (defaults → mounted file → `RANGELIGHT_<SVC>_*`) and `secret://` references. The only env reader. |
| `telemetry` (`rl-telemetry`) | JSON logs through `tracing`. |
| `proto` (`rl-proto`) | Code generated from `api/proto` at build time (needs `buf`). |
| `tickets-linear` (`rl-tickets-linear`) | The Linear `TicketSource`. |
| `testkit` (`rl-testkit`) | Fakes, the `TicketSource` contract suite, a Linear stub, real Postgres, NATS and Dex, and a browser for sign-in. Tests only. |

Tests that need Postgres, NATS or Dex run under `scripts/test/services.sh run …`, which starts pinned,
checksum-verified binaries; `scripts/check.sh rust` does that for you. SQL compiles against the
committed `.sqlx/`; after changing a query or a migration, refresh it with
`scripts/test/services.sh run bash -c 'cargo sqlx migrate run --source db/migrations && cargo sqlx prepare --workspace -- --all-targets'`.
