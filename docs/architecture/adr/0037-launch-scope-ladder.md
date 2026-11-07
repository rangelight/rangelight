# 0037. First build and public launch: "Orca in Kubernetes"

- Status: accepted (revised 2026-10-31; replaces the earlier v0.1–v0.3 cut)
- Date: 2026-10-31
- Deciders: the owner
- Amends: ADR 0003 (the earlier MVP becomes later rungs), ADR 0025/0026 (go public at this cut)

## Decision

**Launch (v0.1):**
- A Linear ticket appears in the Workbench → **Start**.
- The Go runner creates a namespace plus one agent container from a **pre-baked image** (toolchain and dependencies), with a worktree.
- The agent feed, terminal, diff, logs and status are all live in the Workbench, with a fleet of sessions side by side.
- The session ends in a GitHub PR, linked back on the Linear ticket.

**Components in the launch:**
- the Go runner and the `Session` CRD
- the TS agent image: Agent SDK, event stream, a real PTY, PR creation
- a pre-baked image per repo (BuildKit, tagged by digest)
- a tiny key-injecting gateway
- the Rust `api` (Postgres event log, state and outbox with RLS; Linear; Connect streaming)
- NATS as a single server
- Dex with the GitHub connector
- the Workbench, built from the mockup
- a Helm chart with a k3d quickstart

**Estimate:** about 29–46 working days, most likely about 6 weeks at a previous project's pace.

**Then, in order:**
- v0.2: Argo neighbours and the Service swap ("copy of prod")
- v0.3: the signed receipt
- v0.4: quotas, priority and preemption, the warm pool, Valkey, OpenBao
- v0.5+: plan sessions, fork/rewind, the MCP gateway and token broker, Cedar packs, Merkle checkpoints, …

**Positioning:** "Orca/Conductor for Kubernetes: parallel agents, each in its own namespace, for platform teams." Orca (MIT) is a clean-room UX reference only.

## Consequences

- It's a strict subset that uses the same contracts, so later rungs are additive (ADR 0025).

## Amendment (external review, 2026-11-01)

**The launch cut is revised to "wedge + focused UI"** (owner, after the external review). It replaces "Orca in Kubernetes".

**Launch:**
1. A Linear ticket or GitHub issue arrives.
2. Rangelight creates a namespace with the agent plus the **Argo neighbours from the OTel Demo** (the copy of prod, ADRs 0019 and 0014).
3. It opens a PR with a **cosign-signed in-toto receipt** (ADR 0022), carrying the session chain head.
4. It records a **per-session cost line** from the gateway's metering (ADR 0034).
5. **A focused Workbench:** one excellent session view with the feed, terminal, diff and preview link.

**Later rungs:**
- next: the multi-session fleet polish, quotas, priority and preemption, the warm pool
- then: everything else

**Realistic estimate:** 10–14 weeks, per the review.

**The launch must also include** the "Must design before code" items in the decisions log, in particular the git mirror and seeding.

**Positioning:** "Every ticket gets its own copy of prod. Every PR comes with a receipt." It is no longer "Orca for Kubernetes".

**Kept in the launch** (owner, 2026-11-01, overriding the review's deferral advice):
- Valkey (ADR 0032)
- per-role column grants (ADR 0030)
- Merkle/WORM org checkpoints, with their verifier (ADR 0029)
- plan sessions (ADR 0007)

Estimate impact: about +2–3 weeks, so **about 12–17 weeks** in total.

Risk: the WORM claims depend on RustFS Object Lock passing the POC (ADR 0030 amendment).

## Amendment (second external review, 2026-11-02)

- **This ADR no longer defines launch scope.** `ARCHITECTURE.md` §10 does (ADR 0044, item 1).
- **v0.1.0 (design-partner demo):** the team dispatcher plus per-ticket workspaces, the thin receipt, and the fleet Workbench. Argo is not in it.
- **v0.2.0 (public alpha):** adds the copy of prod (Argo neighbours, Service swap, previews) and fixtures-based preview data. The four extras above are ship-unless-slip for v0.2.0.
