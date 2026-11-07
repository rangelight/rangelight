# 0040. Launch plumbing: terminal, webhooks, GitHub App, stream authz, image builds, jobs, upgrades, mirror

- Status: accepted
- Date: 2026-11-01
- Deciders: the owner
- Closes items 4–11 of "Must design before code".

## Decision

1. **Terminal (PTY):**
   - Path: browser (xterm.js) ↔ a WebSocket on the `api` ↔ the NATS subjects `pty.<session>.in` and `pty.<session>.out` ↔ the agent runner's real PTY.
   - The WebSocket upgrade requires OIDC and a Cedar check.
   - Human keystrokes are recorded as sealed events (ADR 0030).
2. **Webhooks:**
   - Polling is the default in the quickstart: Linear and GitHub every 30s, with ETags.
   - Webhooks switch on automatically when the `api` has a public URL.
   - A tunnel (cloudflared) is documented, not bundled.
3. **GitHub App:**
   - Created through the **manifest flow** from the Workbench setup wizard.
   - Its private key lives in a Kubernetes Secret (OpenBao from v0.4), readable only by the `api` and the push job.
   - Permissions include Checks and Pull requests (read and write) and the `pull_request` event, for verifying any branch (ADR 0048).
4. **Workbench → session authorization:**
   - Every stream (feed, logs, terminal, diff) goes through the `api` with the OIDC session.
   - A Cedar check runs on every stream open.
   - There is no direct path from the browser to a pod, NATS or the gateway.
5. **Image builds:**
   - **Rootless BuildKit** runs in the Rangelight system namespace, never in session namespaces.
   - It does only trusted builds from `main`: prebuilds and devcontainers (ADR 0023).
   - It pushes to **Zot**, which is a launch subchart.
   - Builds are triggered by a push to `main` or by hand.
   - Rootless BuildKit's seccomp and AppArmor needs are checked in the POC.
6. **Durable timers and jobs:**
   - A Postgres job table with `run_at` and `SKIP LOCKED`, written in the same transaction as state (ADR 0031).
   - It handles autostop, Decision expiry, retries and golden-copy refreshes (ADR 0038).
   - NATS stays transport only.
7. **Upgrades:**
   - `helm upgrade`.
   - Migrations run as a pre-upgrade hook Job and follow **expand/contract only**.
   - Running sessions keep running through protocol skew N-1..N (ADR 0027).
   - **CNPG Barman backups to RustFS**, with a restore drill tested in CI.
8. **The git mirror** (stock git with hooks, ADR 0011) is part of the launch (ADR 0037).

## Amendment (2026-11-06)

- **The GitHub App also subscribes to `installation` and `installation_repositories`**, which drive repo onboarding (ADR 0047).
