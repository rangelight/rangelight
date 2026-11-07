# 0033. API: Connect-RPC over the shared protos; no CLI in the MVP

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner

## Decision

- **The public API is Connect-RPC**, defined by the same Buf schemas as the runner and session protocols.
  - **Server:** `connectrpc/connect-rust` (official, Apache-2.0, passes the conformance suite, pre-1.0 so pinned). It serves Connect, gRPC and gRPC-Web.
  - **Client:** `connect-es` in the browser.
  - **Live feeds:** server-streaming RPCs. The control plane fans them out from NATS, so the browser never connects to NATS.
  - **JSON over HTTP POST** works with `curl`.
  - **Browser auth:** OIDC session cookies.
  - A REST/OpenAPI surface can be added later by transcoding the same protos.
- **No CLI in the MVP** (owner's call). Existing tools cover each job:
  - **verify:** `cosign verify-blob-attestation` with the org public key, plus a `jq`/`yq` script that checks the predicate and replays the hash chain. The script is published and tested in CI against signed fixtures, including a tampered one.
  - **lint:** `check-jsonschema` against the published schemas.
  - **preflight:** a Helm pre-install hook Job.

## Revisit if

- Users ask for terminal workflows. Then build the CLI on the same Connect protos, or offer the Rangelight MCP server.

## Amendment (2026-11-04)

- **The Rangelight MCP server is in v0.1.0** (ADR 0048): a remote `/mcp` endpoint on the `api`, so local agents can hand work to Rangelight. There is still no CLI.
