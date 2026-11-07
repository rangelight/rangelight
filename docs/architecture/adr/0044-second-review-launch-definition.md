# 0044. Second external review: v0.1 dispatcher + workspaces, v0.2 copy of prod, fixtures first, adoption gates

- Status: accepted
- Date: 2026-11-02
- Deciders: the owner for items 1–3 and 11; my calls for items 4–10.
- Review: `reviews/2026-11-02-external-review.md`; response: `reviews/2026-11-02-review-response.md`
- Amends: ADRs 0014, 0019, 0020, 0022, 0026, 0029, 0032, 0034, 0037, 0038, 0042, 0043

## Decision

1. **Two milestones, one launch definition** (owner). ADRs 0037 and 0042 no longer define launch scope. `ARCHITECTURE.md` §10 is the single source.
   - **v0.1.0: the design-partner demo.** It is not public. It has:
     - the team **dispatcher** (item 11)
     - **workspaces:** one namespace and hardened pod per Linear ticket, with a pre-baked image and a worktree from the local git mirror. The worker agent is one SDK session, plus an optional reviewer subagent (item 6).
     - a workspace starts only when a member applies a label or changes the ticket's state (§7 #22)
     - Linear sync-back: status, comments, and the PR link
     - the PR, through the `api`'s push job
     - **the thin receipt:**
       - DSSE/in-toto
       - the per-session hash chain
       - the offline events bundle and the verify script
       - **one observed gate Job** that runs the repo's tests on the committed SHA
     - the cost line and hard token caps
     - the Workbench: a **fleet view** (dispatcher chat plus the workspace lanes) and the one-session drill-in
     - Grill-lite, the core config pipeline, hot slots with a queue and P0
     - every open threat-model §7 item that doesn't depend on Argo, previews or preview data
   - **v0.2.0: the public alpha.** It adds:
     - **the copy of prod:** Argo neighbours, the Service swap (ADRs 0014, 0019), and previews on a separate domain (forward-auth, wildcard TLS, host-only cookies)
     - preview data from **fixtures**, with external deps set to mock, sandbox or block
     - §7 items #1, #2, #17 and #18
     - the real-cluster install and the upgrade/restore gates
     - **Ship-unless-slip:** Valkey, plan sessions, column grants, Merkle/WORM org checkpoints. These are planned and built for v0.2.0. They're dropped last-in, first-out only if the go/no-go before v0.2.0 hardening shows the adoption gates slipping.
   - Nothing outside these lists is in either milestone.
2. **Preview data is fixtures first** (owner):
   - v0.2.0 supports `fixtures`, plus external deps set to `mock`, `sandbox` or `block`.
   - `masked-snapshot` is the **first capability after v0.2.0**. It ships only after its full lifecycle is designed and tested: build, fail-closed masking, PII scan, clone, reset, migrate, destroy, restore, and the UX for a failed or partial copy.
   - §7 items 19 and 20 move with it and become its entry gates.
   - The Plan 1 Greenmask/TopoLVM check is dropped from the POC. It returns with that capability.
   - The schema keeps the `masked-snapshot` value. In v1alpha1 the `api` rejects it with "not supported in this release".
3. **Design-partner soft gate** (owner):
   - The v0.1 demo is shown to at least one platform team outside the owner's company, and their feedback is written into the Plan 1 report before Plan 2 is written.
   - Building isn't blocked while a partner is found.
   - Going public (v0.2.0) needs a **fresh install by a non-owner**.
4. **Adoption gates replace week estimates as the measure of progress** (my call):

   | # | Gate | Milestone |
   | --- | --- | --- |
   | 1 | a fresh k3d install by a non-owner, following the README only | v0.1 |
   | 2 | an install on one real cluster (RKE2 or EKS) | v0.2 |
   | 3 | GitHub App setup through the manifest flow | v0.1 |
   | 4 | ticket → PR on the OTel Demo | v0.1 |
   | 5 | the receipt verified offline | v0.1 |
   | 6 | recovery from a failed session: an agent crash, a cancel, and a runner restart mid-session | v0.1 |
   | 7 | `helm upgrade` from the previous alpha tag with sessions running, plus one backup restore | v0.2 |

   The 15–22-week figure in ADR 0042 is kept only as a sanity check.
5. **State authorities, restated** (my call; ADR 0032's "NATS KV holds presence, leases, live session state" contradicted ADR 0021 and is withdrawn):

   | Store | Holds | Never holds |
   | --- | --- | --- |
   | Postgres | product truth: state, events (`seq` assigned by `relay`, the single writer), outbox, jobs, quota ledger | – |
   | `Session` CR | the runner's runtime projection, rebuilt from `SessionDesired` | anything not derivable from Postgres |
   | NATS JetStream | transport only, with at-least-once delivery. Consumers dedupe on `(session, event id)`. | state; no KV buckets |
   | Valkey | counters, caches, idempotency TTLs. Every key is rebuildable or disposable. | anything durable |
   | Argo (v0.2) | the rendered neighbours, desired from the runner's Application | product state |
   | Git | `rangelight-config` and the code | runtime state |

   - **Replay:** after a crash, `relay` resumes from the JetStream consumer's ack floor.
   - **Dedupe:** duplicates are dropped on a unique `(session_id, event_id)` constraint.
   - **Presence and leases:** if needed, they're Postgres rows with TTL columns.
6. **Worker execution model** (my call; tightens the ADR 0020 amendment):
   - one SDK session per workspace, using the SDK's own resume and auto-compaction
   - the digest is injected on resume and after compaction
   - **one optional reviewer subagent**
   - Parallel lanes are separate workspaces under the dispatcher (item 11), not in-pod subagents.
7. **The launch gateway is bounded** (my call; restates the ADR 0034 amendment):
   - It does auth (projected token), key injection by reference, Anthropic SSE passthrough with metering, the host allowlist, and hard token caps.
   - In v0.1, caps use per-session counters in Postgres, which allows only one gateway replica. From v0.2 they use Valkey counters, unless Valkey is cut.
   - **Not in v0.1 or v0.2:** Bedrock and Vertex signing, the MCP OAuth broker, web CONNECT egress, in-gateway Cedar.
   - When the MCP broker is built, it targets the 2026-07-28 spec's client order: pre-registered, then **Client ID Metadata Documents**, then DCR (deprecated) as a fallback.
8. **No CLI, for real** (my call): the leftover `rangelight verify` references in ADRs 0022 and 0029 now mean the verify script from ADR 0033 (`cosign verify-attestation` plus a jq script), and ADR 0026 no longer lists a CLI.
9. **Launch design gaps get an owning plan** (my call). Each one is designed in that plan's first task, before any code. See the decisions log, "Launch design gaps".
10. **Competitors are written down, not hand-waved** (my call). `PRODUCT.md` gains a positioning section with dated, sourced notes on:
    - Coder Agents (GA on 2026-09-09; the closest overlap)
    - the GitHub Copilot coding agent
    - Signadot

    Ona/Gitpod, Daytona, E2B, Bosun and kagent get notes before going public.
11. **The team dispatcher** (owner):
    - **What it is:** one long-lived Agent SDK session per Linear team (the "architect"), in its own pod.
    - **It never touches code.** Its only tools are Rangelight API calls:
      - list tickets and workspaces
      - start or stop a workspace
      - read a workspace's status and summary
      - message a worker
      - propose (a Decision card or a ticket comment)
    - **It knows the state by querying Postgres every turn, not by remembering it.** Compaction and restarts lose nothing.
    - **It has its own Cedar role, `dispatcher`:**
      - Allowed: start, stop and message workspaces for its own team, and only on tickets a member labelled.
      - Never allowed: approving Decision cards, pushing, or merging.
      - Its actions are rate-limited.
    - **Linear sync-back** is the `api`'s job, not the dispatcher's: workspace status, comments and the PR link.

## Pushed back

- **"Combine the gateway with the API, or use one backend language."** Rejected.
  - The gateway runs per cluster, in the session egress path. The `api` is central. Merging them couples the agent's blast radius to the control plane.
  - The language split is by boundary: Rust for the control plane and gateway, Go for Kubernetes, TS for the SDK and the browser. That's in `ARCHITECTURE.md` §3.
- **"Defer Valkey, plan sessions, column grants, Merkle/WORM."** The owner kept them for the second time. Item 1 makes them honest, as ship-unless-slip for v0.2.0, rather than ambiguous.
- **"Launch with copy of prod, agent, PR and receipt together."** Accepted, as v0.2.0. v0.1.0 alone (ticket-driven agents in isolated pods) sits in Coder Agents' and Copilot's territory, so it stays a private design-partner demo and isn't the public launch. It ships first because it proves the runner, mirror, gateway, receipt and dispatcher without the hardest unproven part (previews and prod-shaped data).

## Consequences

- The POC drops Argo and Greenmask. Both return in v0.2.
- The roadmap is split into v0.1.0 and v0.2.0 milestones, with adoption gates per milestone.
- `otel-demo.rangelight.yaml` uses fixtures for `astronomy-db`. Its `neighbours` and `park` blocks are read only from v0.2.
- In v0.1, the service under change runs inside the workspace pod (hot reload) and is reached through a port-forward. There is no preview URL.
