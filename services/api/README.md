# api

The control plane's front door (ADR 0031, ADR 0033):

- **Linear intake.** Every 30 seconds it lists the configured team's open issues labelled
  `rangelight` and creates one session per issue, in the configured workspace (`repo`, `base_ref`,
  `member`). Session, first event and outbox row are written in one transaction; the
  `UNIQUE (tracker, ticket_id)` constraint makes a re-poll a no-op. An issue that breaks the ticket
  rule (`api/jsonschema/ticket.v1alpha1.schema.json`: its criteria are the description's `AC<n>`
  lines and the numbered items under an "Acceptance criteria" heading), or a command that fails, is
  logged and skipped; the round goes on. A skipped issue is recorded in `ticket_skips` with a hash of its
  title, description and reason, and the link round comments on it once per hash, saying why
  (RAN-64): a reply or a status change is no new version. A Linear failure is retried next round; an
  issue no longer listed as skipped (fixed, closed or unlabelled) loses its skip, so a fixed one
  never hears it was skipped. At start the api warns when the team has no `rangelight` label.
- **Runner status.** A durable JetStream consumer (`api`, stream `rl-<org>-status`, which it
  creates) of every runner's `rl.<org>.runner.<runner>.status`. Each `SessionStatus` appends one
  `session.phase_changed` event and sets the session's phase in one transaction, and is acked only
  after it commits; `UNIQUE (events.message_id)` makes a redelivery a no-op. A message that can
  never be recorded is terminated, with its reason logged.
- **Pull requests** (RAN-56). A status's `pull_request_url` is kept only when it is
  `https://github.com/<owner>/<repo>/pull/<n>` on `upstream`, the configured repo's GitHub repo,
  and the session works in that repo; anything else is ignored and logged with the session id only.
  A session keeps its first one: it is stored, recorded on that report's event, and served as
  `Session.pull_request_url`. After each intake round, a link round attaches every unlinked one to
  its Linear issue (`attachmentCreate`, idempotent by URL) and records `pull_request_linked_at`; a
  Linear failure is logged and retried next round, and the stored URL stays.
- **Session API**, Connect-RPC (`api/proto/rangelight/session/v1alpha1/session.proto`), on
  `listen` (8080): `ListSessions`, `GetSession`, and the server streams `WatchSession` (one
  session) and `WatchSessions` (a snapshot of every session, then each change), fed by the relay's
  NATS fan-out (`rl.<org>.session.<id>.events`), never by polling Postgres. Each subscribes before
  it reads Postgres. A seq gap (a lost event, or a viewer more than 1024 events behind: the
  viewers' NATS client's subscription capacity) ends the stream with `ABORTED`.
- **Lane terminal** (ADR 0040 §1), a WebSocket at `GET /v1alpha1/sessions/<id>/terminal`, subprotocol
  `terminal.v1alpha1.rangelight.dev` (else 400), for a session that exists (else 404) and is the
  signed-in member's own (else 403: the agent acts for its member, ADR 0004), at most 4 per login
  (else 429). Each text frame
  is held to `api/jsonschema/terminal.v1alpha1.schema.json` and goes unchanged to the agent on core
  NATS `rl.<org>.pty.<id>.in`; one that isn't an `input` or `resize` (binary and oversized ones too)
  closes the socket with 1008. Valid `output` frames from `rl.<org>.pty.<id>.out` go to the browser;
  anything else the lane sends is dropped. Frames are never logged, at any level (ADR 0046 §3 step 4):
  a log line names the session and why the socket closed, nothing more. What a terminal holds in
  the api is bounded: its output queues at most 64 NATS messages (more are dropped), and a browser
  that doesn't take a frame within 10 seconds is closed with 1011.
- **Sign-in** (ADR 0028, ADR 0033), OIDC against bundled Dex or any provider (`shared/rust/auth`):
  `GET /auth/login` starts the authorization-code flow with PKCE (S256), `state` (also in the
  cookie `__Host-rl_login`, so a callback only works in the browser that started it) and `nonce`.
  `?prompt=consent`, the one `prompt` it takes (any other is 400), asks the provider for it: the
  Workbench's Sign in after a sign-out, so bundled Dex asks the member to grant access instead of
  signing them straight back in (its GitHub connector passes no prompt on to GitHub);
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
  A long-lived response (`WatchSession`, `WatchSessions`, the terminal WebSocket) re-checks its login
  every `login_recheck_seconds` (60) and ends once it is signed out, expired or idle: a stream with
  `UNAUTHENTICATED`, the terminal with close code 1008 (`Auth::ended`; an open stream doesn't count
  as use).
- **Members** (AC5 of RAN-37): the member is the ID token's `oidc_member_claim`, by default
  `preferred_username` (the GitHub login, through Dex), lowercased, which is also the sessions'
  `member` label value. A value that isn't `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$` is refused, never
  rewritten. The first account (issuer and subject) to sign in as a member owns it; another account
  with the same name is refused. With your own provider the claim must be one the provider sets and
  keeps unique, never user-editable (OIDC Core warns against keying on `preferred_username`).
- **Who may sign in** is the provider's decision: Dex lets in the members of `dex.github.org`; your
  own provider must restrict who can get a token for this client, or anyone it knows gets in.
- **NATS's auth callout** (ADR 0021 amendment, `src/nats_auth/`). Every NATS client, the api's own
  connections included, signs in with its projected ServiceAccount token. On `$SYS.REQ.USER.AUTH` (queue group
  `rangelight-api`, so replicas share it) the api verifies the token against its issuer's JWKS
  (`token.rs`: RS256 or ES256, audience `rangelight-nats`, expiry; keys cached, fetched again for an unknown
  `kid` at most every 10 s), maps it to one identity (`identity.rs`: the api and the relay in
  `rangelight-system`, a runner by its ServiceAccount, a lane by its `agent` pod in the `rl-` namespace of a live session its
  runner reported Starting or Running, one connection each, its sign-ins one at a time: `lane.rs`), and answers with a user JWT signed by the issuer nkey (`jwt.rs`) holding exactly that identity's
  subjects and limits (`permissions.rs`), for at most an hour. Anything else gets a refusal with a reason; the
  token never reaches a log. Its own connection signs in with the `auth_user` nkey seed (or `.creds` file).
  At start the api waits for a NATS it can't reach, trying again up to every 5 s and logging each try
  (`rl_nats::connect`); a refused sign-in or any other start failure still stops it.
- **HTTP/1.1 only** (Connect and the terminal WebSocket need nothing else). A client gets 10
  seconds to send a request's headers, from connecting and between requests on a kept-alive
  connection; a connection that sends nothing is closed.
- It applies the migrations in `db/migrations/` at startup. `GET /healthz` answers 200, without a login, once
  it serves: after the migrations, Postgres and NATS.

Config: `configs/api.default.toml`, then `/etc/rangelight/api.toml`, then `RANGELIGHT_API_*`.
`linear_team`, `public_url` (`https://<host>[:<port>]`), `oidc_issuer` (https), `repo` and `member` have no
default; `linear_url` must be https; `upstream` is empty (no pull requests kept) or a GitHub repo URL;
`nats_url` is `tls://` or `nats://` to a Service in the cluster (the token travels there). Secrets are references:
`database_url`, `linear_api_key`, `oidc_client_secret`, `nats_auth.issuer_seed` and `nats_auth.auth_user` name
mounted Secret files (`secret://<name>#<key>` → `/var/run/rangelight/secrets/<name>/<key>`). The Linear key is read
at each poll and never logged.

Not here yet: authorisation (Cedar): every signed-in member sees every session.

```sh
scripts/check.sh rust                                         # everything, with Postgres, NATS and Dex
scripts/test/services.sh run cargo nextest run -p api         # just these tests
docker build -f build/package/docker/api/Dockerfile .         # the image
```

Code: `src/lib.rs` (server, NATS start), `src/intake.rs` (intake, link and comment rounds, label check), `src/sessions.rs` (handlers; `src/sessions/terminal.rs`, the
terminal), `src/auth.rs` (sign-in; `src/auth/recheck.rs`, long-lived responses), `src/status.rs` (runner status),
`src/nats_auth/` (the NATS auth callout), `src/config.rs`, `src/main.rs` (wiring). Tests: `tests/api.rs`, end to end
through a Linear stub, the relay and a real Connect client; `tests/auth.rs`, sign-in through the test Dex;
`tests/nats_auth/`, every identity's permissions on a nats-server running the chart's `templates/nats/_auth.conf.tpl`;
`tests/terminal/`, a real WebSocket client and NATS.
