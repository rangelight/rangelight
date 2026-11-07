# 0004. Credential custody: no raw credential in the agent pod; delegated user identity

- Status: accepted. Which LLM credential types are allowed (API key vs subscription) is pending research and will be its own ADR.
- Date: 2026-10-29
- Deciders: the owner

## Decision

- **No raw credential ever enters an agent pod.** This covers LLM keys, subscription tokens, MCP OAuth tokens and git tokens.
- **The pod's only identity** is a short-lived workload token bound to `agent:<session>/<lane>`:
  - `lean`: a projected ServiceAccount token.
  - `scale`: a SPIFFE SVID.
- **Egress is default-deny**, except to Rangelight's gateways.
- **The LLM gateway** (via `ANTHROPIC_BASE_URL`):
  - resolves a credential *reference* from the secret store
  - injects it into the call
  - meters the call into the usage ledger
  - enforces the budget
- **The MCP gateway:**
  - The user connects an MCP once, in the browser. Rangelight is the OAuth client.
  - Refresh tokens are envelope-encrypted, using KMS or OpenBao/Vault transit.
  - The gateway enforces the approved-tool allowlist.
  - It injects the token per call.
  - It logs the tool, the scope and the credential *ref* (never the value) into the receipt.
  - stdio MCP servers that need secrets run beside the gateway, not in the agent pod.
- **Git:** only the broker holds the GitHub App token (see the git ADR).
- **Agent identity on external MCPs is delegated and scoped.**
  - The agent acts on behalf of the session owner's grant, narrowed by the allowlist.
  - The audit line reads: `agent:<session>/<lane> on behalf of <user>`.
  - Team service credentials are allowed only for shared infra tools that admins approve explicitly.

## Consequences

- A leaked pod environment contains nothing reusable.
- The SaaS tools' own permissions and audit stay truthful.
- The residual risk is a *confused deputy*: the agent misusing a delegated capability through the gateway.
  - Mitigations at the gateway: read vs. write scopes, rate limits, and a Decision card for writes outside the contract.
- Unattended runs need the owner's grant to still be valid. An expired grant raises a Decision card.

## Revisit if

- A tool supports only static keys. Then wrap it as a service credential behind the same gateway.
- MCP enterprise-managed authorization (token exchange through the org IdP) matures enough to replace per-user browser consent.

## Amendment (external review, 2026-10-31)

- **Subscription tokens are removed from gateway custody.** Anthropic's terms forbid third parties from collecting, storing or intermediating Claude.ai credentials (code.claude.com/docs/en/legal-and-compliance).
- **The gateway injects only org-owned credentials:** API keys, Bedrock, Vertex and Foundry.
- **Personal-seat mode** may only mean a per-session login in the unmodified `claude` CLI. It can't use the Agent SDK runner (ADR 0020). Status: parked, low priority.
- **The hosted tier must be strictly bring-your-own-key or bring-your-own-seat.** It can't bundle or resell inference.
- **Until OpenBao (v0.4+), credentials are Kubernetes Secret references** behind the same credential-reference interface.

## Amendment (2026-11-06)

- **Credential modes are ADR 0046.** Team API key and plan API credits are injected by the gateway. Plan sign-in (gated on Anthropic's confirmation) is the one narrow exception to "no raw credential in a pod": the member's own Claude credential, created by Claude Code's own flow inside their lane's pod, never seen by Rangelight, destroyed with the pod. This replaces the "personal-seat mode … can't use the Agent SDK runner" line above.
