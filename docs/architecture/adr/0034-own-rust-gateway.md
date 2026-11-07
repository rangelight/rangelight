# 0034. Our own Rust gateway for all session egress

- Status: accepted
- Date: 2026-10-31
- Deciders: the owner. The owner chose to build it rather than adopt agentgateway.
- Amends:
  - ADR 0004 and 0016: the gateway is ours.
  - ADR 0032: the Envoy ratelimit server is dropped; the gateway uses Valkey directly.

## Decision

- **One Rust gateway** is the only egress for session pods, apart from the git mirror and the package caches.
- **Base:** Pingora or hyper/Tower. The builder decides; Tower matches connect-rust.
- **LLM traffic:**
  - Anthropic Messages passthrough, including SSE streaming.
  - Token usage is read from the stream (`message_delta` usage) for metering and budgets.
  - Bedrock requests are signed with SigV4, Vertex requests with Google auth.
  - Credentials are injected by reference from OpenBao. The agent never sees a key.
- **MCP traffic:**
  - MCP spec 2026-07-28 (stateless, Streamable HTTP), on the official Rust MCP SDK (to verify).
  - The gateway is the OAuth client to upstream servers, with no token passthrough, as the spec requires.
  - **The token broker is part of the gateway:** consent, plus refresh tokens wrapped with OpenBao Transit.
  - A pooled stdio runner hosts approved local MCP servers.
- **Web egress:**
  - An HTTP CONNECT proxy with a **host allowlist** (SNI and CONNECT authority must match). **No TLS interception in the MVP.**
  - Per-session additions come through Decision cards.
  - The receipt records host, bytes and duration.
- **Policy:**
  - In-process Cedar (the same crate as the control plane), with a policy snapshot pushed over NATS.
  - Budgets and rate limits are checked against Valkey counters.
  - Usage events go to NATS, and then to the ledger and the receipt.
- **Hardening before going public:**
  - fuzzing (protocol parsers, SSE and MCP framing)
  - a conformance suite (MCP and Anthropic API contract tests)
  - an external security review

## Revisit if

- The maintenance or security burden exceeds the benefit. agentgateway (Apache-2.0, Linux Foundation) is the drop-in fallback for LLM and MCP.
- Orgs need full-URL audit. Then add an opt-in TLS-terminating mode.

## Amendment (external review, 2026-10-31)

- **ADR 0034 is split by rung:**
  - **v0.1:** a small LLM proxy that injects the key, meters tokens per session into the event log (the first cost line), and allowlists hosts.
  - **v0.5:** the MCP OAuth broker and the web CONNECT egress. Whether those are built by us or adopted (agentgateway) is decided then.
- **The hardening gate** (fuzzing, conformance, external review) applies to whichever rung first exposes each surface publicly. For the v0.1 proxy, that's fuzzing its parsers plus an internal review.
- **Known residual risks of SNI-allowlisting without MITM:**
  - domain fronting through allowlisted CDNs
  - Encrypted Client Hello (ECH) hiding the SNI

  These are documented, not solved.

## Amendment (second external review, 2026-11-02)

- **The v0.1 and v0.2 gateway is bounded** (ADR 0044, item 7): projected-token auth, key injection, Anthropic SSE passthrough with metering, the host allowlist, and hard token caps.
- **Not in v0.1 or v0.2:** Bedrock and Vertex signing, the MCP broker, CONNECT egress, in-gateway Cedar.
- In v0.1, caps use Postgres counters with one gateway replica. From v0.2 they use Valkey, unless Valkey is cut.
- **MCP client registration:** the 2026-07-28 spec deprecates DCR in favour of Client ID Metadata Documents. Order: pre-registered, then CIMD, then DCR.

## Amendment (2026-11-06)

- **Plan sign-in lanes bypass the gateway** for model traffic and go direct to Anthropic (ADR 0046). Their caps are Anthropic's plan limits; the cost line shows plan usage.
