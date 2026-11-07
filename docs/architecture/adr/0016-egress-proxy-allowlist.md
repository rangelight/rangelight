# 0016. Egress: agents reach only in-cluster Rangelight endpoints; the web goes through an allowlisted proxy

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner

## Decision

- **Agent pods may reach only in-cluster Rangelight endpoints:**
  - the LLM gateway
  - the MCP gateway
  - the git mirror
  - the package pull-through caches
  - the egress proxy

  Plain Kubernetes NetworkPolicy enforces this. No CNI FQDN feature is required. Cilium is recommended but optional.
- **Packages:** npm, PyPI, Go, Maven, NuGet, crates and container images go through in-cluster pull-through caches (area Q).
- **General web:** goes through an allowlisted HTTP(S) egress proxy in the gateway layer.
  - An org allowlist in config (ADR 0005) enables documentation sites by default.
  - Per-session additions need a Decision card.
  - Every request is recorded in the receipt.
  - Fetched content is tagged as untrusted input to the model.
- **Only the gateways, caches and proxy have outbound egress.** Each one has its own allowlist.

## Consequences

- One choke point per traffic class, which gives audit and policy for free.
- Prompt injection from the web is contained to allowlisted hosts and labelled.

## Revisit if

- Allowlist friction measurably hurts agent success. Then consider a curated "docs mirror" or a search-API tool.

## Amendment (external review, 2026-10-31)

- **NATS (ADR 0021) is added to the agent pod's allowed endpoints.** The pod uses session-scoped credentials limited to its own subjects.
- **At launch, the pod's endpoints are:** the LLM gateway, the git mirror and NATS.
- **Packages:** while area Q is parked, package installs beyond the pre-baked dependencies go through the egress allowlist to the public registries. This is an explicit, logged, temporary rule.

## Amendment (2026-11-01): superseded in part by ADR 0041

- **The agent pod's launch endpoints are:**
  - the LLM gateway
  - the git mirror
  - NATS
  - **git-pkgs/proxy**, the read-only package proxy
  - **Zot**
- **The temporary rule allowing public registries is withdrawn.** Package hosts are no longer on the egress allowlist.
