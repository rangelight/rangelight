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
- **Sign-in** (ADR 0028, ADR 0033), OIDC against bundled Dex or any provider (`shared/rust/auth`):
  `GET /auth/login` starts the authorization-code flow with PKCE (S256), `state` (also in the
  cookie `__Host-rl_login`, so a callback only works in the browser that started it) and `nonce`;
  `GET /auth/callback` takes the sign-in once, exchanges the code, verifies the ID token (signature
  via discovery and JWKS, issuer, audience, expiry, nonce), and sets `__Host-rl_session`
  (`HttpOnly; Secure; SameSite=Lax; Path=/`): a random 256-bit token whose SHA-256 is the
  `logins` row. A login ends after 8 hours unused or 7 days in all. `POST /auth/logout` deletes it.
  A sign-in in progress is stored nowhere: `state`, `nonce` and the PKCE verifier travel in
  `__Host-rl_login`, sealed with AES-256-GCM under HKDF-SHA256 of the client secret, valid 10
  minutes, so `/auth/login` writes nothing. The client secret must hold at least 32 bytes
  (`openssl rand -base64 48`); the api refuses to start with a shorter one. Signing in again ends the browser's previous login.
- **Every RPC and WebSocket upgrade** needs a live login, else 401 `unauthenticated` with nothing
  more. Anything but a plain GET (every RPC, sign-out, every upgrade) must carry
  `Origin: <public_url>`, else 403 `permission_denied`. `AuthService.WhoAmI` names the member.
  A long-lived response (`WatchSession`, later the terminal WebSocket) re-checks its login every
  `login_recheck_seconds` (60) and ends with `UNAUTHENTICATED` once it is signed out, expired or
  idle (`Auth::while_signed_in`; an open stream doesn't count as use).
- **Members** (AC5 of RAN-37): the member is the ID token's `oidc_member_claim`, by default
  `preferred_username` (the GitHub login, through Dex), lowercased, which is also the sessions'
  `member` label value. A value that isn't `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$` is refused, never
  rewritten. The first account (issuer and subject) to sign in as a member owns it; another account
  with the same name is refused. With your own provider the claim must be one the provider sets and
  keeps unique, never user-editable (OIDC Core warns against keying on `preferred_username`).
- **Who may sign in** is the provider's decision: Dex lets in the members of `dex.github.org`; your
  own provider must restrict who can get a token for this client, or anyone it knows gets in.
- **HTTP/1.1 only** (Connect and the coming terminal WebSocket need nothing else). A client gets 10
  seconds to send a request's headers, from connecting and between requests on a kept-alive
  connection; a connection that sends nothing is closed.
- It applies the migrations in `db/migrations/` at startup.

Config: `configs/api.default.toml`, then `/etc/rangelight/api.toml`, then `RANGELIGHT_API_*`.
`linear_team`, `public_url` (`https://<host>[:<port>]`) and `oidc_issuer` (https) have no default;
`linear_url` must be https. Secrets are references: `database_url`, `linear_api_key` and `oidc_client_secret` name
mounted Secret files (`secret://<name>#<key>` → `/var/run/rangelight/secrets/<name>/<key>`). The
Linear key is read at each poll and never logged.

Not here yet: authorisation (Cedar): every signed-in member sees every session.

```sh
scripts/check.sh rust                                         # everything, with Postgres, NATS and Dex
scripts/test/services.sh run cargo nextest run -p api         # just these tests
docker build -f build/package/docker/api/Dockerfile .         # the image
```

Code: `src/lib.rs` (intake loop, server), `src/sessions.rs` (handlers), `src/auth.rs` (sign-in; `src/auth/recheck.rs`, long-lived responses),
`src/config.rs`, `src/main.rs` (wiring). Tests: `tests/api.rs`, end to end through a Linear stub,
the relay and a real Connect client; `tests/auth.rs`, sign-in through the test Dex.
