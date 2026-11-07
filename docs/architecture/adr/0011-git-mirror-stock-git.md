# 0011. Git mirror: stock git, no custom broker

- Status: accepted. The owner challenged the "Go git broker" leaning.
- Date: 2026-10-30

## Decision

We don't develop a git server. Each requirement maps to stock parts:

| Need | Stock mechanism |
| --- | --- |
| Serve clone/fetch/push in-cluster (session pods, Argo CD) | `git http-backend` (smart HTTP) behind nginx |
| AuthN: who is calling | nginx `auth_request` to Rangelight, which validates the pod's projected SA token (ADR 0004) and maps it to the session and its granted repos |
| AuthZ per repo | the same `auth_request`, per path |
| AuthZ per ref (a session may only push `refs/rangelight/sessions/<id>/*` and its own branches) | a `pre-receive` hook that checks `REMOTE_USER` against the ref prefixes |
| Checkpoint events into the event log (ADR 0010) | a `post-receive` hook that POSTs to Rangelight |
| Freshness from upstream | the GitHub App webhook triggers `git fetch`, plus a periodic fetch |
| Push upstream and open/update the PR | a **control-plane job**, not the mirror. It is policy-checked, receipt-logged, and uses the GitHub App installation token, which never leaves the control plane |

- The agent never holds an upstream credential.
- The mirror holds no secret beyond its webhook verification.

## Consequences

- No custom git server code. The mirror is an nginx + git image and a few hook scripts.
- Hooks are product code. They need tests, a CI self-check, and versioning with Rangelight.

## Revisit if

- Huge monorepos need protocol v2 partial-clone tuning, bundle-URIs or packfile caching beyond what stock git offers.
- We need SSH transport.

## Amendment (2026-11-06)

- **Upstream fetches are a control-plane Job** with a short-lived installation token from a credential helper, never the mirror Deployment (ADR 0047 §3). This resolves the conflict between "holds no secret" and "runs `git fetch`". Fetches use explicit refspecs and never prune `refs/rangelight/*`.
