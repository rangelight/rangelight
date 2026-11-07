# 0017. In-tree integrations and open protocols; no plugin runtime

- Status: accepted
- Date: 2026-10-30
- Deciders: the owner
- Supersedes: ADR 0006 in part (the WASM component runtime and the executable marketplace)

## Context

The owner asked whether we need a plugin system at all, given that the number of integrations is finite.

Sorted by cardinality:
- **Bounded categories (about 20 implementations in total):**
  - trackers (about 6)
  - forges (about 5)
  - GitOps engines (2)
  - notifiers (about 5, plus a generic webhook)
- **The unbounded long tail is already covered:**
  - agent tools (MCP)
  - gates (containers emitting the evidence schema)
  - agent adapters (the session protocol)
  - skills, policy packs, presets and templates (data)

Precedent: Renovate keeps its forge platforms in-tree, and Argo CD Notifications keeps about 25 services in-tree.

## Decision

- **Bounded categories:** compiled-in implementations of interfaces/traits (`Tracker`, `Forge`, `GitOpsEngine`, `Notifier`).
  - Contributions are upstream PRs that get our tests and our security review.
- **In-house systems:** one in-tree **generic HTTP-contract adapter** per category, with a documented REST and webhook spec that internal systems implement (the ApplicationSet plugin-generator approach).
- **The long tail stays on open protocols:**
  - MCP, for tools
  - containers emitting the evidence schema, for gates
  - the session protocol, for agents
- **The signed public catalog (Scryer model, ADR 0006) holds only non-executable artifacts:**
  - skills
  - policy packs
  - preview presets
  - pipeline templates
  - approved MCP server manifests

  It keeps Sigstore signer pinning, the tiers and OCI distribution.
- **No WASM host and no plugin ABI.**

## Consequences

- The language decision no longer depends on WASM hosting.
- No WIT versioning, conformance infrastructure or capability-diff reviews for executable plugins.
- Integration code is trusted. The hosted multi-tenant tier needs no sandbox for it.
- Third parties can't ship an executable integration without upstream review. That is accepted.

## Revisit if

- A category's long tail appears (for example dozens of requested trackers).
- Customers need private executable logic that the HTTP contract can't express.
- The hosted tier needs untrusted per-tenant code.
